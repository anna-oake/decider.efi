use log::{info, warn};
use uefi::prelude::*;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::boot;

use crate::config::Choice;
use crate::helpers::{parse_choice, read_text_file};

const CHOICE_PATH: &str = "\\DECIDER.CHO";

pub fn find_fs_choice(image_handle: Handle) -> Result<Choice, Status> {
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
