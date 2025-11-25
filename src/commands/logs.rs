use crate::cli::LogsArgs;
use crate::container::runtime::{ContainerRuntime, RuntimeConfig};
use crate::error::Result;
use crate::CuboError;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::Duration;
use tracing::warn;

pub async fn execute(args: LogsArgs) -> Result<()> {
    let config = RuntimeConfig::from_env();
    let runtime = ContainerRuntime::new(config.clone())?;

    let container = runtime.get_container(&args.container).await?;
    let log_path = get_log_path(&config.root_dir, &container.id);
    if !log_path.exists() {
        println!("No logs available for container {}", args.container);
        return Ok(());
    }
    if args.follow {
        follow_logs(&log_path, args.timestamps).await?;
    } else {
        print_logs(&log_path, args.tail, args.timestamps)?;
    }

    Ok(())
}

fn get_log_path(root_dir: &PathBuf, container_id: &str) -> PathBuf {
    root_dir.join(container_id).join("container.log")
}


fn print_logs(log_path: &PathBuf, tail: Option<usize>, timestamps: bool) -> Result<()> {
    let file = File::open(log_path)
        .map_err(|e| CuboError::SystemError(format!("Failed to open log file: {}", e)))?;
    
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();

    let lines_to_print = if let Some(n) = tail {
        if lines.len() > n {
            &lines[lines.len() - n..]
        } else {
            &lines[..]
        }
    } else {
        &lines[..]
    };

    for line in lines_to_print {
        if timestamps {
            println!("{}", line);
        } else {
            if let Some(msg) = strip_timestamp(&line) {
                println!("{}", msg);
            } else {
                println!("{}", line);
            }
        }
    }
    Ok(())
}

async fn follow_logs(log_path: &PathBuf, timestamps: bool) -> Result<()> {
    let mut file = File::open(log_path)
        .map_err(|e| CuboError::SystemError(format!("Failed to open log file: {}", e)))?;

    file.seek(SeekFrom::End(0))
        .map_err(|e| CuboError::SystemError(format!("Failed to seek: {}", e)))?;

    let mut reader = BufReader::new(file);
    let mut line = String::new();

    loop {
        match reader.read_line(&mut line) {
            Ok(0) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
                line.clear();
            }
            Ok(_) => {
                let output = if timestamps {
                    line.clone()
                } else {
                    strip_timestamp(&line).unwrap_or(line.clone())
                };
                print!("{}", output);
                line.clear();
            }
            Err(e) => {
                warn!("Error reading log file: {}", e);
                break;
            }
        }
    }
    Ok(())
}

fn strip_timestamp(line: &str) -> Option<String> {
    if let Some(pos) = line.find(char::is_whitespace) {
        if pos > 20 && pos < 35 {
            return Some(line[pos..].trim_start().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;
    use crate::container::Container;

    #[test]
    fn test_get_log_path() {
        let root = PathBuf::from("/var/lib/cubo");
        let container_id = "abc123";
        let path = get_log_path(&root, container_id);
        assert_eq!(path, PathBuf::from("/var/lib/cubo/abc123/container.log"));
    }

    #[test]
    fn test_string_timestamp_withtimestamp() {
        let line = "2025-11-24T20:30:00.123456Z Hello world\n";
        let result = strip_timestamp(line);
        assert_eq!(result, Some("Hello world\n".to_string()))
    }

    #[test]
    fn test_strip_timestamp_without_timestamp() {
        let line = "Hello world\n";
        let result = strip_timestamp(line);
        assert_eq!(result, None);
    }

    #[test]
    fn test_strip_timestamp_short_line() {
        let line = "Hi\n";
        let result = strip_timestamp(line);
        assert_eq!(result, None);
    }

    #[test]
    fn test_print_logs_basic() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("container.log");
        let mut file = File::create(&log_path).unwrap();
        writeln!(file, "Line 1").unwrap();
        writeln!(file, "Line 2").unwrap();
        writeln!(file, "Line 3").unwrap();
        print_logs(&log_path, None, false)?;
        Ok(())
    }

    #[test]
    fn test_print_logs_with_tail() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("container.log");
        let mut file = File::create(&log_path).unwrap();
        for i in 1..=10 {
            writeln!(file, "Line {}", i).unwrap();
        }
        print_logs(&log_path, Some(3), false)?;
        Ok(())
    }

    #[test]
    fn test_print_logs_with_timestamps() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("container.log");
        let mut file = File::create(&log_path).unwrap();
        writeln!(file, "2025-11-24T20:30:00.123456Z Line 1").unwrap();
        writeln!(file, "2025-11-24T20:30:01.123456Z Line 2").unwrap();

        print_logs(&log_path, None, true)?;
        Ok(())
    }

    #[test]
    fn test_print_logs_tail_larger_than_file() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("container.log");
        let mut file = File::create(&log_path).unwrap();
        writeln!(file, "Line 1").unwrap();
        writeln!(file, "Line 2").unwrap();
        writeln!(file, "Line 3").unwrap();
        print_logs(&log_path, Some(100), false)?;
        Ok(())
    }

    #[test]
    fn test_print_logs_empty_file() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("container.log");
        File::create(&log_path).unwrap();
        print_logs(&log_path, None, false)?;
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_container_not_found() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("CUBO_ROOT", temp_dir.path().to_string_lossy().to_string());

        let args = LogsArgs {
            container: "nonexistant".to_string(),
            follow: false,
            tail: None,
            timestamps: false,
        };

        let result = execute(args).await;
        assert!(result.is_err());
        std::env::remove_var("CUBO_ROOT");
    }

    #[tokio::test]
    async fn test_execute_no_logs() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig{
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string(), "hello".to_string()]
        );
        let container_id = runtime.create_container(container).await.unwrap();
        std::env::set_var("CUBO_ROOT", temp_dir.path().to_string_lossy().to_string());
        let args = LogsArgs {
            container: container_id.clone(),
            follow: false,
            tail: None,
            timestamps: false,
        };

        let result = execute(args).await;
        assert!(result.is_ok());
        std::env::remove_var("CUBO_ROOT");
    }

    #[tokio::test]
    async fn test_execute_with_logs() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string(), "hello".to_string()],
        );
        let container_id = runtime.create_container(container).await.unwrap();
        let log_path = get_log_path(&temp_dir.path().to_path_buf(), &container_id);
        fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let mut file = File::create(&log_path).unwrap();
        writeln!(file, "Test log line 1").unwrap();
        writeln!(file, "Test log line 2").unwrap();

        std::env::set_var("CUBO_ROOT", temp_dir.path().to_string_lossy().to_string());
        let args = LogsArgs {
            container: container_id.clone(),
            follow: false, 
            tail: None,
            timestamps: false,
        };
        let result = execute(args).await;
        assert!(result.is_ok());
        std::env::remove_var("CUBO_ROOT");
    }

}