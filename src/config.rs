use alloc::string::String;
use alloc::borrow::ToOwned;
use alloc::vec::Vec;
use log::info;
use uefi::prelude::*;
use uefi::boot;

use crate::helpers::{
    get_optional_value, get_required_value, normalize_uefi_path, parse_key_values, read_text_file,
};

pub const CONFIG_PATH: &str = "\\decider\\decider.conf";

#[derive(Debug)]
pub struct Config {
    pub chainload_systemd_path: String,
    pub chainload_mode_paths: Vec<(String, String)>,
    pub choice_source: ChoiceSource,
    pub tftp_ip: Option<String>,
}

#[derive(Debug)]
pub enum Choice {
    Entry(String),
    NixosCurrent,
    Chainload(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceSource {
    Fs,
    Tftp,
}

impl Config {
    pub fn chainload_path_for_mode(&self, mode: &str) -> Option<&str> {
        let mode_key = alloc::format!("{}_path", mode);
        self.chainload_mode_paths
            .iter()
            .find(|(key, _)| key == &mode_key)
            .map(|(_, value)| value.as_str())
    }
}

pub fn load_config(image_handle: Handle) -> Result<Config, Status> {
    let mut esp_fs = boot::get_image_file_system(image_handle).map_err(|e| e.status())?;
    let text = read_text_file(&mut esp_fs, CONFIG_PATH)?;
    info!("loaded config from {}", CONFIG_PATH);
    parse_config(&text)
}

fn parse_config(text: &str) -> Result<Config, Status> {
    let kv = parse_key_values(text);
    let chainload_systemd_path = normalize_uefi_path(get_required_value(&kv, "chainload_systemd_path")?);
    let choice_source = match get_optional_value(&kv, "choice_source").unwrap_or("fs") {
        "fs" => ChoiceSource::Fs,
        "tftp" => ChoiceSource::Tftp,
        _ => return Err(Status::LOAD_ERROR),
    };
    let tftp_ip = get_optional_value(&kv, "tftp_ip").map(ToOwned::to_owned);

    if choice_source == ChoiceSource::Tftp && tftp_ip.is_none() {
        return Err(Status::LOAD_ERROR);
    }

    let mut chainload_mode_paths = Vec::new();
    for (key, value) in &kv {
        if key.starts_with("chainload_") && key.ends_with("_path") && key != "chainload_systemd_path" {
            chainload_mode_paths.push((key.clone(), normalize_uefi_path(value)));
        }
    }

    Ok(Config {
        chainload_systemd_path,
        chainload_mode_paths,
        choice_source,
        tftp_ip,
    })
}
