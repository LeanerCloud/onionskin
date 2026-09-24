//! The Edit menu's image entries, acting on the image the Edit Image tool
//! selected.
//!
//! Turning and flipping are commands `tools-edit` registers, so the menu
//! carries only their ids. Replacing and saving need a file the user picks,
//! which only the shell can ask for, so those two are shell behaviour.

use onionskin_plugin_api::command_ids as ids;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ImageCommand {
    RotateClockwise,
    RotateCounterclockwise,
    FlipHorizontal,
    FlipVertical,
    Replace,
    SaveAs,
}

impl ImageCommand {
    pub(in crate::shell) const ALL: [Self; 6] = [
        Self::RotateClockwise,
        Self::RotateCounterclockwise,
        Self::FlipHorizontal,
        Self::FlipVertical,
        Self::Replace,
        Self::SaveAs,
    ];

    /// The registered command this entry runs, or `None` for the two the
    /// shell runs itself.
    pub(in crate::shell) fn registry_id(self) -> Option<&'static str> {
        match self {
            Self::RotateClockwise => Some(ids::ROTATE_IMAGE_CLOCKWISE),
            Self::RotateCounterclockwise => Some(ids::ROTATE_IMAGE_COUNTERCLOCKWISE),
            Self::FlipHorizontal => Some(ids::FLIP_IMAGE_HORIZONTAL),
            Self::FlipVertical => Some(ids::FLIP_IMAGE_VERTICAL),
            Self::Replace | Self::SaveAs => None,
        }
    }

    pub(in crate::shell) fn id(self) -> &'static str {
        match self {
            Self::Replace => "edit.replace-image",
            Self::SaveAs => "edit.save-image-as",
            registered => registered.registry_id().unwrap_or_default(),
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::RotateClockwise => "Rotate Image Clockwise",
            Self::RotateCounterclockwise => "Rotate Image Counterclockwise",
            Self::FlipHorizontal => "Flip Image Horizontal",
            Self::FlipVertical => "Flip Image Vertical",
            Self::Replace => "Replace Image…",
            Self::SaveAs => "Save Image As…",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_entries_are_registered_commands_and_two_are_the_shells() {
        let registered: Vec<_> = ImageCommand::ALL
            .into_iter()
            .filter_map(ImageCommand::registry_id)
            .collect();
        assert_eq!(registered.len(), 4);
        for command in ImageCommand::ALL {
            assert!(!command.id().is_empty(), "{command:?}");
            assert!(command.label().contains("Image"), "{command:?}");
        }
        let mut ids: Vec<_> = ImageCommand::ALL.map(ImageCommand::id).to_vec();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ImageCommand::ALL.len(), "distinct ids");
    }
}
