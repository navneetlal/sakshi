use sakshi_capture::Source;
use sakshi_core::{DataDir, MeetingDir, MeetingInfo, write_json_atomic};

use crate::AppError;

/// A new folder under `<data>/meetings/<id>/` for a recording of `source`,
/// with its `meeting.json` (id, title, start time) written.
pub fn create_meeting(
    data: &DataDir,
    source: &Source,
    title: Option<String>,
) -> Result<(MeetingDir, MeetingInfo), AppError> {
    let id = uuid::Uuid::now_v7().to_string();
    let meeting = data.meeting(&id);
    std::fs::create_dir_all(meeting.path()).map_err(|e| AppError::io(meeting.path(), e))?;
    let title = title
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| default_title(source));
    let info = MeetingInfo {
        id: Some(id),
        title: Some(title),
        started_at: Some(jiff::Timestamp::now().to_string()),
        language: None,
    };
    write_json_atomic(&meeting.info_json(), &info)?;
    Ok((meeting, info))
}

fn default_title(source: &Source) -> String {
    match (&source.meeting, source.title.trim()) {
        (Some(app), "") => app.clone(),
        (_, "") => source
            .app_name
            .clone()
            .unwrap_or_else(|| "Recording".into()),
        (_, title) => title.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use sakshi_capture::SourceId;
    use sakshi_core::read_json;

    use super::*;

    fn source(title: &str, meeting: Option<&str>) -> Source {
        Source {
            id: SourceId::Window(1),
            title: title.into(),
            app_name: Some("zoom.us".into()),
            bundle_id: None,
            width: 100,
            height: 100,
            meeting: meeting.map(str::to_owned),
            on_screen: true,
        }
    }

    #[test]
    fn creates_a_folder_with_meeting_json() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::new(root.path());

        let (meeting, info) =
            create_meeting(&data, &source("Zoom Meeting", Some("Zoom")), None).unwrap();

        assert!(meeting.path().starts_with(data.meetings_dir()));
        let on_disk: MeetingInfo = read_json(&meeting.info_json()).unwrap();
        assert_eq!(on_disk, info);
        assert_eq!(info.title.as_deref(), Some("Zoom Meeting"));
        assert!(uuid::Uuid::parse_str(info.id.as_deref().unwrap()).is_ok());
    }

    #[test]
    fn titles_fall_back_sensibly() {
        assert_eq!(default_title(&source("", Some("Zoom"))), "Zoom");
        assert_eq!(default_title(&source("  ", None)), "zoom.us");
        let root = tempfile::tempdir().unwrap();
        let (_, info) = create_meeting(
            &DataDir::new(root.path()),
            &source("x", None),
            Some("  Weekly sync ".into()),
        )
        .unwrap();
        assert_eq!(info.title.as_deref(), Some("Weekly sync"));
    }
}
