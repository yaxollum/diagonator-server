use crate::time::HourMinute;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::fs;
use std::path::PathBuf;
use toml_edit::easy as toml;

#[derive(Serialize, Deserialize, Debug)]
pub struct RequirementConfig {
    pub name: String,
    pub due: HourMinute,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LockedTimeRangeConfig {
    pub start: Option<HourMinute>,
    pub end: Option<HourMinute>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DiagonatorConfig {
    pub bind_on: String,
    pub requirements: Option<Vec<RequirementConfig>>,
    pub locked_time_ranges: Option<Vec<LockedTimeRangeConfig>>,
    pub work_period_minutes: i64,
    pub break_minutes: i64,
}

impl Default for DiagonatorConfig {
    fn default() -> Self {
        Self {
            bind_on: "0.0.0.0:3000".to_owned(),
            requirements: Some(vec![
                RequirementConfig {
                    name: "Name of requirement 1".to_owned(),
                    due: HourMinute::new(8, 30).unwrap(),
                },
                RequirementConfig {
                    name: "Name of requirement 2".to_owned(),
                    due: HourMinute::new(20, 00).unwrap(),
                },
            ]),
            locked_time_ranges: Some(vec![
                LockedTimeRangeConfig {
                    start: None,
                    end: Some(HourMinute::new(4, 30).unwrap()),
                },
                LockedTimeRangeConfig {
                    start: Some(HourMinute::new(12, 00).unwrap()),
                    end: Some(HourMinute::new(13, 00).unwrap()),
                },
                LockedTimeRangeConfig {
                    start: Some(HourMinute::new(22, 00).unwrap()),
                    end: None,
                },
            ]),
            work_period_minutes: 25,
            break_minutes: 5,
        }
    }
}

#[derive(Debug)]
pub enum LoadConfigError {
    ConfigDirNotFound,
    SerializationError(toml::ser::Error),
    DeserializationError(toml::de::Error),
    WriteError(PathBuf, std::io::Error),
    ReadError(PathBuf, std::io::Error),
    CreateDirError(PathBuf, std::io::Error),
}

impl Display for LoadConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigDirNotFound => {
                write!(f, "Unable to determine path to configuration directory")
            }
            Self::SerializationError(err) => {
                write!(f, "Received error '{}' when serializing configuration", err)
            }
            Self::DeserializationError(err) => {
                write!(
                    f,
                    "Received error '{}' when deserializing configuration",
                    err
                )
            }
            Self::WriteError(path, err) => {
                write!(
                    f,
                    "Received error '{}' when writing to file {}",
                    err,
                    path.display()
                )
            }
            Self::ReadError(path, err) => {
                write!(
                    f,
                    "Received error '{}' when reading from file {}",
                    err,
                    path.display()
                )
            }
            Self::CreateDirError(path, err) => {
                write!(
                    f,
                    "Received error '{}' when creating directory {}",
                    err,
                    path.display()
                )
            }
        }
    }
}
impl From<toml::ser::Error> for LoadConfigError {
    fn from(err: toml::ser::Error) -> Self {
        Self::SerializationError(err)
    }
}

impl From<toml::de::Error> for LoadConfigError {
    fn from(err: toml::de::Error) -> Self {
        Self::DeserializationError(err)
    }
}

fn make_default_config(config_file_path: &PathBuf) -> Result<(), LoadConfigError> {
    eprintln!(
        "Creating default configuration file at {}",
        config_file_path.display()
    );
    let contents = toml::to_string_pretty(&DiagonatorConfig::default())?;
    fs::write(config_file_path, contents)
        .map_err(|err| LoadConfigError::WriteError(config_file_path.clone(), err))
}

pub fn load_config() -> Result<DiagonatorConfig, LoadConfigError> {
    let mut config_file_path = dirs::config_dir().ok_or(LoadConfigError::ConfigDirNotFound)?;
    config_file_path.push("diagonator-server");
    fs::create_dir_all(&config_file_path)
        .map_err(|err| LoadConfigError::CreateDirError(config_file_path.clone(), err))?;
    config_file_path.push("config.toml");
    if !config_file_path.exists() {
        make_default_config(&config_file_path)?;
    }
    eprintln!("Loading configuration from {}", config_file_path.display());
    let contents = fs::read_to_string(&config_file_path)
        .map_err(|err| LoadConfigError::ReadError(config_file_path, err))?;

    let config = toml::from_str(&contents)?;
    Ok(config)
}
