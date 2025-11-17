use std::fs;
use std::path::Path;

use crate::error::{CuboError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    Base { image: String },
    /// RUN <command> - execute command in container
    Run { command: String },
    /// COPY <src> <dest> - copy files from build context to container
    Copy { src: String, dest: String },
    /// ENV <key>=<value> - set environment variable
    Env { key: String, value: String },
    /// WORKDIR <path> - set working directory
    Workdir { path: String },
    /// CMD <command> - default command to run
    Cmd { command: Vec<String> },
    /// Comment or empty line (ignored)
    Comment,
}

#[derive(Debug, Clone)]
pub struct Cubofile {
    pub instructions: Vec<Instruction>,
}

impl Cubofile {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .map_err(|e| CuboError::SystemError(format!("Failed to read Cubofile: {}", e)))?;
        Self::from_string(&content)
    }

    pub fn from_string(content: &str) -> Result<Self> {
        let mut instructions = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let line_num = line_num + 1;
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                instructions.push(Instruction::Comment);
                continue;
            }

            let instruction = Self::parse_line(trimmed, line_num)?;
            instructions.push(instruction);
        }

        Ok(Self { instructions })
    }

    fn parse_line(line: &str, line_num: usize) -> Result<Instruction> {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.is_empty() {
            return Ok(Instruction::Comment);
        }

        let directive = parts[0].to_uppercase();
        let args = if parts.len() > 1 { parts[1].trim() } else { "" };

        match directive.as_str() {
            "BASE" => {
                if args.is_empty() {
                    return Err(CuboError::InvalidConfiguration(format!(
                        "Line {}: BASE requires an image argument",
                        line_num
                    )));
                }
                Ok(Instruction::Base {
                    image: args.to_string(),
                })
            }

            "RUN" => {
                if args.is_empty() {
                    return Err(CuboError::InvalidConfiguration(format!(
                        "Line {}: RUN requires a command",
                        line_num
                    )));
                }
                Ok(Instruction::Run {
                    command: args.to_string(),
                })
            }

            "COPY" => {
                let copy_parts: Vec<&str> = args.split_whitespace().collect();
                if copy_parts.len() != 2 {
                    return Err(CuboError::InvalidConfiguration(format!(
                        "Line {}: COPY requires exactly 2 arguments: <src> <dest>",
                        line_num
                    )));
                }
                Ok(Instruction::Copy {
                    src: copy_parts[0].to_string(),
                    dest: copy_parts[1].to_string(),
                })
            }

            "ENV" => {
                if let Some(eq_pos) = args.find('=') {
                    let key = args[..eq_pos].trim().to_string();
                    let value = args[eq_pos + 1..].trim().to_string();
                    if key.is_empty() {
                        return Err(CuboError::InvalidConfiguration(format!(
                            "Line {}: ENV key cannot be empty",
                            line_num
                        )));
                    }
                    Ok(Instruction::Env { key, value })
                } else {
                    Err(CuboError::InvalidConfiguration(format!(
                        "Line {}: ENV must be in format KEY=value",
                        line_num
                    )))
                }
            }

            "WORKDIR" => {
                if args.is_empty() {
                    return Err(CuboError::InvalidConfiguration(format!(
                        "Line {}: WORKDIR requires a path",
                        line_num
                    )));
                }
                Ok(Instruction::Workdir {
                    path: args.to_string(),
                })
            }

            "CMD" => {
                if args.is_empty() {
                    return Err(CuboError::InvalidConfiguration(format!(
                        "Line {}: CMD requires a command",
                        line_num
                    )));
                }
                // Parse as shell command (split by whitespace)
                let cmd_parts: Vec<String> = args.split_whitespace().map(|s| s.to_string()).collect();
                Ok(Instruction::Cmd { command: cmd_parts })
            }

            _ => Err(CuboError::InvalidConfiguration(format!(
                "Line {}: Unknown directive: {}",
                line_num, directive
            ))),
        }
    }

    /// Get the base image (first BASE instruction)
    pub fn base_image(&self) -> Option<String> {
        for instruction in &self.instructions {
            if let Instruction::Base { image } = instruction {
                return Some(image.clone());
            }
        }
        None
    }

    /// Get all RUN instructions
    pub fn run_commands(&self) -> Vec<String> {
        self.instructions
            .iter()
            .filter_map(|inst| {
                if let Instruction::Run { command } = inst {
                    Some(command.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_base() {
        let content = "BASE alpine:latest";
        let cubofile = Cubofile::from_string(content).unwrap();
        assert_eq!(cubofile.instructions.len(), 1);
        assert_eq!(
            cubofile.instructions[0],
            Instruction::Base {
                image: "alpine:latest".to_string()
            }
        );
    }

    #[test]
    fn test_parse_run() {
        let content = "RUN apk add curl";
        let cubofile = Cubofile::from_string(content).unwrap();
        assert_eq!(
            cubofile.instructions[0],
            Instruction::Run {
                command: "apk add curl".to_string()
            }
        );
    }

    #[test]
    fn test_parse_copy() {
        let content = "COPY ./app /usr/bin/app";
        let cubofile = Cubofile::from_string(content).unwrap();
        assert_eq!(
            cubofile.instructions[0],
            Instruction::Copy {
                src: "./app".to_string(),
                dest: "/usr/bin/app".to_string()
            }
        );
    }

    #[test]
    fn test_parse_env() {
        let content = "ENV PATH=/usr/bin";
        let cubofile = Cubofile::from_string(content).unwrap();
        assert_eq!(
            cubofile.instructions[0],
            Instruction::Env {
                key: "PATH".to_string(),
                value: "/usr/bin".to_string()
            }
        );
    }

    #[test]
    fn test_parse_workdir() {
        let content = "WORKDIR /app";
        let cubofile = Cubofile::from_string(content).unwrap();
        assert_eq!(
            cubofile.instructions[0],
            Instruction::Workdir {
                path: "/app".to_string()
            }
        );
    }

    #[test]
    fn test_parse_cmd() {
        let content = "CMD /bin/sh -c echo";
        let cubofile = Cubofile::from_string(content).unwrap();
        assert_eq!(
            cubofile.instructions[0],
            Instruction::Cmd {
                command: vec!["/bin/sh".to_string(), "-c".to_string(), "echo".to_string()]
            }
        );
    }

    #[test]
    fn test_parse_full_cubofile() {
        let content = r#"
# This is a comment
BASE alpine:latest

RUN apk add --no-cache curl
RUN apk add git

COPY ./myapp /usr/local/bin/myapp
ENV PATH=/usr/local/bin:$PATH
WORKDIR /app
CMD /usr/local/bin/myapp serve
"#;

        let cubofile = Cubofile::from_string(content).unwrap();
        assert_eq!(cubofile.base_image(), Some("alpine:latest".to_string()));
        assert_eq!(cubofile.run_commands().len(), 2);
    }

    #[test]
    fn test_parse_case_insensitive() {
        let content = "base alpine:latest\nrun echo hello";
        let cubofile = Cubofile::from_string(content).unwrap();
        assert!(matches!(cubofile.instructions[0], Instruction::Base { .. }));
        assert!(matches!(cubofile.instructions[1], Instruction::Run { .. }));
    }

    #[test]
    fn test_invalid_directive() {
        let content = "INVALID directive";
        let result = Cubofile::from_string(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_base_arg() {
        let content = "BASE";
        let result = Cubofile::from_string(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_env_format() {
        let content = "ENV NOEQUALS";
        let result = Cubofile::from_string(content);
        assert!(result.is_err());
    }
}
