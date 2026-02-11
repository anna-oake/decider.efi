use alloc::borrow::ToOwned;
use alloc::string::String;
use log::{info, warn};
use uefi::prelude::*;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::boot;

use crate::helpers::{
    get_required_value, normalize_uefi_path, parse_key_values, read_text_file,
};

pub const CONFIG_PATH: &str = "\\decider\\decider.conf";
const CHOICE_PATH: &str = "\\DECIDER.CHO";

#[derive(Debug)]
pub struct Config {
    pub chainload_path: String,
}

#[derive(Debug)]
pub enum Choice {
    Entry(String),
    NixosCurrent,
}

pub fn load_config(image_handle: Handle) -> Result<Config, Status> {
    let mut esp_fs = boot::get_image_file_system(image_handle).map_err(|e| e.status())?;
    let text = read_text_file(&mut esp_fs, CONFIG_PATH)?;
    info!("loaded config from {}", CONFIG_PATH);
    parse_config(&text)
}

pub fn find_choice(image_handle: Handle) -> Result<Choice, Status> {
    let source_device = boot::open_protocol_exclusive::<LoadedImage>(image_handle)
        .ok()
        .and_then(|img| img.device());

    let handles = boot::find_handles::<SimpleFileSystem>().map_err(|e| e.status())?;
    info!("discovered {} filesystem handles", handles.len());

    for (idx, handle) in handles.into_iter().enumerate() {
        if source_device == Some(handle) {
            continue;
        }

        let mut fs = match boot::open_protocol_exclusive::<SimpleFileSystem>(handle) {
            Ok(fs) => fs,
            Err(err) => {
                warn!("skipping fs #{} (open failed: {:?})", idx, err.status());
                continue;
            }
        };

        let marker_text = match read_text_file(&mut fs, CHOICE_PATH) {
            Ok(text) => text,
            Err(Status::NOT_FOUND) => continue,
            Err(err) => {
                warn!("skipping fs #{} (read failed: {:?})", idx, err);
                continue;
            }
        };

        info!("found choice file on fs #{}", idx);
        return parse_choice(&marker_text);
    }

    Err(Status::NOT_FOUND)
}

fn parse_config(text: &str) -> Result<Config, Status> {
    let kv = parse_key_values(text);
    let chainload_path = normalize_uefi_path(get_required_value(&kv, "chainload_path")?);
    Ok(Config { chainload_path })
}

fn parse_choice(text: &str) -> Result<Choice, Status> {
    let kv = parse_key_values(text);
    let choice_type = get_required_value(&kv, "choice_type")?;

    match choice_type {
        "entry_id" => Ok(Choice::Entry(get_required_value(&kv, "entry_id")?.to_owned())),
        "nixos-current" => Ok(Choice::NixosCurrent),
        _ => Err(Status::LOAD_ERROR),
    }
}
