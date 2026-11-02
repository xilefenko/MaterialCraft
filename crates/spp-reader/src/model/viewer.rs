//! [`PainterViewer`]: what the project's viewer settings (`/editor/viewersettings.ini`, object serialisation read with
//! the generic walker) say about the environment map. A faithful mirror; offering the map to the user's library is
//! `translate::library`'s job.
//!
//! Layout (Observed on corpus projects): root `ViewerSettingsData` with `materialSettings` (`SettingsMaterial`),
//! whose `cubemapUrl` names the environment map: a shelf URL (`resource://viewer/panorama?version=<id>.image` for
//! Painter's own environments) whose image Painter embeds under `/resources/<id>`. The other viewer fields
//! (exposure, blur, rotation, shaders) are not mirrored.

use super::RawTree;
use crate::decode::DecodeError;

/// Where the viewer settings live inside a `.spp`.
pub const VIEWER_INI: &str = "/editor/viewersettings.ini";

/// The viewer settings this crate reads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PainterViewer {
    /// `materialSettings.cubemapUrl`; `None` when absent or empty.
    pub environment_url: Option<String>,
}

impl PainterViewer {
    pub fn parse(data: &[u8]) -> Result<PainterViewer, DecodeError> {
        let tree = RawTree::parse(data)?;
        let url = tree
            .root
            .as_ref()
            .and_then(|root| root.object("materialSettings"))
            .and_then(|m| m.string("cubemapUrl"))
            .filter(|u| !u.is_empty())
            .map(str::to_string);
        Ok(PainterViewer { environment_url: url })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::document::test_writer::DocWriter;

    fn file(url: &str) -> Vec<u8> {
        let mut w = DocWriter::new(2);
        w.object(0x12, "ViewerSettingsData", 1);
        w.field("materialSettings");
        w.object(0x14, "SettingsMaterial", 1);
        w.field("cubemapUrl")
            .raw(&[0x10])
            .raw(&(url.len() as u32).to_le_bytes())
            .raw(url.as_bytes());
        w.close();
        w.close();
        w.bytes
    }

    #[test]
    fn reads_the_environment_url() {
        let url = "resource://viewer/panorama?version=ab12.image";
        assert_eq!(
            PainterViewer::parse(&file(url)).unwrap().environment_url.as_deref(),
            Some(url)
        );
        assert_eq!(PainterViewer::parse(&file("")).unwrap(), PainterViewer::default());
        assert!(PainterViewer::parse(&[1, 2, 3]).is_err());
    }
}
