use bollard::Docker;
use bollard::container::ListContainersOptions;

use clap::Parser;
use dialoguer::{theme::ColorfulTheme, FuzzySelect, Input, Select};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;
#[derive(Parser, Debug)]
#[command(name = "docker-shell", author, version, about = "Interactively select a Docker container and open a shell")]
struct Args {
    /// Show interactive menus for unspecified options
    #[arg(short, long)]
    custom: bool,

    /// Shell to use (bash, sh, zsh, etc.)
    #[arg(short, long)]
    shell: Option<String>,

    /// User mode: default, host, root, or user:group
    #[arg(short, long)]
    user: Option<String>,

    /// Working directory (default for container's working dir)
    #[arg(short, long)]
    workdir: Option<String>,

    /// Name or ID of the target container
    container: Option<String>,

    /// Run a new container from an image instead of entering an existing one
    #[arg(short, long)]
    run: bool,
}

struct ImageInfo {
    display: String,
    tag: String,
}

struct ContainerInfo {
    name: String,
    image: String,
    working_dir: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    // 1. Connect to Docker Daemon
    let docker = Docker::connect_with_local_defaults()?;
    
    // 2. Branch: --run mode vs exec mode
    if args.run {
        return run_container_mode(&args, &docker).await;
    }
    
    // 3. Fetch running containers natively
    let running_containers = get_running_containers(&docker).await?;
    
    if running_containers.is_empty() {
        eprintln!("❌ No running Docker containers found.");
        std::process::exit(1);
    }

    // 4. Determine Container Target
    let selected_container_name = match args.container {
        Some(name) => name,
        None => {
            // Interactive fuzzy finder selection
            let container_items: Vec<String> = running_containers
                .iter()
                .map(|c| format!("{} ({})", c.name, c.image))
                .collect();

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Select a container to enter:")
                .default(0)
                .items(&container_items)
                .interact()?;
            
            running_containers[selection].name.clone()
        }
    };

    // Find details for the chosen container (or pull defaults if provided via arg)
    let chosen_info = running_containers
        .iter()
        .find(|c| c.name == selected_container_name || c.name == format!("/{}", selected_container_name))
        .map(|c| (c.working_dir.clone(), c.name.clone()))
        .unwrap_or_else(|| ("/".to_string(), selected_container_name));

    let container_default_workdir = if chosen_info.0.is_empty() { "/" .to_string() } else { chosen_info.0 };
    let target_container = chosen_info.1;

    // 5. Determine Shell Selection
    let final_shell = match &args.shell {
        Some(s) => s.clone(),
        None if args.custom => {
            let options = vec!["bash", "sh", "Custom shell"];
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select Shell")
                .items(&options)
                .default(0)
                .interact()?;

            match selection {
                0 => "bash".to_string(),
                1 => "sh".to_string(),
                _ => Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter custom shell path")
                    .interact_text()?,
            }
        }
        None => "bash".to_string(),
    };

    // 6. Determine User Mode
    let final_user = match args.user.as_deref() {
        Some(u) => parse_user_mode(u),
        None if args.custom => {
            let options = vec![
                "Default (container default)",
                "Host current user",
                "Root",
                "Custom user:group"
            ];
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select User Mode")
                .items(&options)
                .default(0)
                .interact()?;

            match selection {
                0 => None,
                1 => {
                    let uid = unsafe { libc::getuid() };
                    let gid = unsafe { libc::getgid() };
                    Some(format!("{}:{}", uid, gid))
                }
                2 => Some("0:0".to_string()),
                _ => {
                    let custom: String = Input::with_theme(&ColorfulTheme::default())
                        .with_prompt("Enter user:group (e.g. 1000:1000)")
                        .interact_text()?;
                    Some(custom)
                }
            }
        }
        None => None, // Falls back to Container default
    };

    // 7. Determine Working Directory
    let final_workdir = match args.workdir {
        Some(ref w) if w == "default" => container_default_workdir,
        Some(w) => w,
        None if args.custom => {
            let default_label = format!("Default ({})", container_default_workdir);
            let options = vec![default_label.as_str(), "Root (/)", "Custom path"];
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select Working Directory")
                .items(&options)
                .default(0)
                .interact()?;

            match selection {
                0 => container_default_workdir,
                1 => "/".to_string(),
                _ => Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter working directory path")
                    .interact_text()?,
            }
        }
        None => container_default_workdir,
    };

    // 8. Hand off execution to the native Docker CLI binary
    // Using `execvp` replacing our process gives the shell true TTY interactive pass-through control.
    println!("Connecting to {}...", target_container);
    
    let mut cmd = Command::new("docker");
    cmd.arg("exec").arg("-it");
    
    if let Some(user) = final_user {
        cmd.arg("-u").arg(user);
    }
    
    cmd.arg("-w").arg(final_workdir);
    cmd.arg(target_container);
    cmd.arg(final_shell);

    // Run the process and hand over standard input/output streams
    let mut child = cmd.spawn().expect("Failed to execute docker exec command");
    let status = child.wait().expect("Failed to wait on docker exec child process");

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

async fn get_running_containers(docker: &Docker) -> Result<Vec<ContainerInfo>, Box<dyn std::error::Error>> {
    let options = Some(ListContainersOptions::<String> {
        all: false, // Only running containers
        ..Default::default()
    });

    let containers = docker.list_containers(options).await?;
    let mut info_list = Vec::new();

    for container in containers {
        if let Some(id) = container.id {
            // Inspect container to pull accurate image labels and workdirs natively
            if let Ok(inspect) = docker.inspect_container(&id, None).await {
                let name = container.names.unwrap_or_default().first().cloned().unwrap_or(id);
                let image = inspect.config.as_ref().and_then(|c| c.image.clone()).unwrap_or_default();
                let working_dir = inspect.config.as_ref().and_then(|c| c.working_dir.clone()).unwrap_or_default();
                
                info_list.push(ContainerInfo { name, image, working_dir });
            }
        }
    }
    Ok(info_list)
}

fn parse_user_mode(mode: &str) -> Option<String> {
    match mode {
        "default" => None,
        "root" => Some("0:0".to_string()),
        "host" => {
            let uid = unsafe { libc::getuid() };
            let gid = unsafe { libc::getgid() };
            Some(format!("{}:{}", uid, gid))
        }
        custom => Some(custom.to_string()),
    }
}

async fn list_images(docker: &Docker) -> Result<Vec<ImageInfo>, Box<dyn std::error::Error>> {
    let images = docker.list_images::<String>(None).await?;
    let mut info_list = Vec::new();

    for image in images {
        for tag in image.repo_tags {
            if tag == "<none>:<none>" {
                continue;
            }
            let size_str = format_bytes(if image.size > 0 { image.size as u64 } else { 0 });
            info_list.push(ImageInfo {
                display: format!("{} ({})", tag, size_str),
                tag,
            });
        }
    }
    Ok(info_list)
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1}GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1}MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1}KB", bytes as f64 / 1_000.0)
    } else {
        format!("{}B", bytes)
    }
}

fn build_container_name(workspace_path: &str) -> String {
    let path = Path::new(workspace_path);
    let basename = path
        .file_name()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();

    let sanitized: String = basename
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let mut hasher = Sha256::new();
    hasher.update(workspace_path.as_bytes());
    let result = hasher.finalize();
    let b64 = URL_SAFE_NO_PAD.encode(result);
    let short_hash = &b64[..8.min(b64.len())];

    format!("{}-{}", sanitized, short_hash)
}

async fn run_container_mode(
    args: &Args,
    docker: &Docker,
) -> Result<(), Box<dyn std::error::Error>> {
    let images = list_images(docker).await?;

    if images.is_empty() {
        eprintln!("❌ No Docker images found.");
        std::process::exit(1);
    }

    let selected_image = match &args.container {
        Some(tag) => tag.clone(),
        None => {
            let image_items: Vec<String> = images.iter().map(|i| i.display.clone()).collect();

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Select an image to run:")
                .default(0)
                .items(&image_items)
                .interact()?;

            images[selection].tag.clone()
        }
    };

    let final_shell = match &args.shell {
        Some(s) => s.clone(),
        None if args.custom => {
            let options = vec!["bash", "sh", "Custom shell"];
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select Shell")
                .items(&options)
                .default(0)
                .interact()?;

            match selection {
                0 => "bash".to_string(),
                1 => "sh".to_string(),
                _ => Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter custom shell path")
                    .interact_text()?,
            }
        }
        None => "bash".to_string(),
    };

    let final_user = match args.user.as_deref() {
        Some(u) => parse_user_mode(u),
        None if args.custom => {
            let options = vec![
                "Default (container default)",
                "Host current user",
                "Root",
                "Custom user:group",
            ];
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select User Mode")
                .items(&options)
                .default(0)
                .interact()?;

            match selection {
                0 => None,
                1 => {
                    let uid = unsafe { libc::getuid() };
                    let gid = unsafe { libc::getgid() };
                    Some(format!("{}:{}", uid, gid))
                }
                2 => Some("0:0".to_string()),
                _ => {
                    let custom: String = Input::with_theme(&ColorfulTheme::default())
                        .with_prompt("Enter user:group (e.g. 1000:1000)")
                        .interact_text()?;
                    Some(custom)
                }
            }
        }
        None => None,
    };

    let cwd = std::env::current_dir()?;
    let cwd_str = cwd.to_string_lossy().to_string();

    let final_mount = match &args.workdir {
        Some(ref w) if w == "default" => "/workspace".to_string(),
        Some(w) if w == "none" => String::new(),
        Some(w) => w.clone(),
        None if args.custom => {
            let options = vec!["Don't mount it", "at /workspace", "at custom path"];
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Mount current directory?")
                .items(&options)
                .default(1)
                .interact()?;

            match selection {
                0 => String::new(),
                1 => "/workspace".to_string(),
                _ => Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter mount point path (e.g. /workspace)")
                    .interact_text()?,
            }
        }
        None => "/workspace".to_string(),
    };

    let container_name = build_container_name(&cwd_str);

    println!("Running {}...", selected_image);

    let mut cmd = Command::new("docker");
    cmd.arg("run")
        .arg("-it")
        .arg("--rm");

    if !final_mount.is_empty() {
        cmd.arg("-v")
           .arg(format!("{}:{}", cwd_str, final_mount));
    }

    cmd.arg("--name")
       .arg(container_name);

    if let Some(ref user) = final_user {
        cmd.arg("-u").arg(user);
    }

    cmd.arg(&selected_image)
        .arg(&final_shell);

    let mut child = cmd.spawn().expect("Failed to execute docker run command");
    let status = child.wait().expect("Failed to wait on docker run child process");

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}