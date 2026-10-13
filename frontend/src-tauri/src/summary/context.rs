use crate::database::repositories::summary_context::{
    SummaryContextRepository, SummaryContextScope,
};
use log::{info, warn};
use serde::Serialize;
use sqlx::SqlitePool;

const MEETING_HEADING: &str = "Context for this meeting:";
const PRIORITY_NOTE: &str = "If these conflict, the more specific one takes priority: this meeting first, then the folder, then the general context.";

/// One inherited level of summary context that applies to a meeting.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InheritedContext {
    pub scope: SummaryContextScope,
    /// Empty for the global level, the folder id for a folder level.
    pub scope_id: String,
    /// Folder name for a folder level.
    pub label: Option<String>,
    pub context: String,
}

/// Inherited levels that apply to this meeting, most general first: the global
/// level, then the meeting's own folder. Levels with empty text are left out.
pub async fn load_inherited_contexts(
    pool: &SqlitePool,
    meeting_id: &str,
) -> Result<Vec<InheritedContext>, sqlx::Error> {
    let mut levels = Vec::new();
    levels.extend(inherited_level(pool, SummaryContextScope::Global, "", None).await?);

    let folder: Option<(String, String)> = sqlx::query_as(
        "SELECT f.id, f.name FROM meetings m
         JOIN folders f ON f.id = m.folder_id
         WHERE m.id = ? AND f.deleted_at IS NULL",
    )
    .bind(meeting_id)
    .fetch_optional(pool)
    .await?;
    if let Some((folder_id, folder_name)) = folder {
        levels.extend(
            inherited_level(pool, SummaryContextScope::Folder, &folder_id, Some(folder_name))
                .await?,
        );
    }
    Ok(levels)
}

/// One level's saved text as an inherited context, or nothing when it has no text.
async fn inherited_level(
    pool: &SqlitePool,
    scope: SummaryContextScope,
    scope_id: &str,
    label: Option<String>,
) -> Result<Option<InheritedContext>, sqlx::Error> {
    Ok(SummaryContextRepository::get(pool, scope, scope_id)
        .await?
        .filter(|row| !row.context.is_empty())
        .map(|row| InheritedContext {
            scope,
            scope_id: scope_id.to_string(),
            label,
            context: row.context,
        }))
}

fn heading(level: &InheritedContext) -> String {
    match (level.scope, level.label.as_deref()) {
        (SummaryContextScope::Global, _) => "General context (applies to every meeting):".to_string(),
        (SummaryContextScope::Folder, Some(name)) => format!("Folder context (\"{}\"):", name),
        (SummaryContextScope::Folder, None) => "Folder context:".to_string(),
        (SummaryContextScope::Meeting, _) => MEETING_HEADING.to_string(),
    }
}

/// Builds the text for the `<user_context>` block of the final report prompt.
///
/// With no inherited text the meeting text is returned unchanged, so prompts and
/// cache fingerprints stay identical for meetings that inherit nothing.
pub fn compose_summary_context(inherited: &[InheritedContext], meeting_text: &str) -> String {
    let mut sections: Vec<String> = inherited
        .iter()
        .filter(|level| !level.context.trim().is_empty())
        .map(|level| format!("{}\n{}", heading(level), level.context.trim()))
        .collect();
    if sections.is_empty() {
        return meeting_text.to_string();
    }

    let meeting = meeting_text.trim();
    if !meeting.is_empty() {
        sections.push(format!("{}\n{}", MEETING_HEADING, meeting));
    }

    let mut composed = sections.join("\n\n");
    if sections.len() >= 2 {
        composed.push_str("\n\n");
        composed.push_str(PRIORITY_NOTE);
    }
    composed
}

/// The context text for one summary run. If the inherited levels cannot be loaded
/// the run continues with the meeting text only.
pub async fn resolve_summary_context(
    pool: &SqlitePool,
    meeting_id: &str,
    meeting_text: &str,
    use_inherited: bool,
) -> String {
    let inherited = if use_inherited {
        load_inherited_contexts(pool, meeting_id)
            .await
            .unwrap_or_else(|e| {
                warn!(
                    "Failed to load inherited summary context for meeting {}: {}",
                    meeting_id, e
                );
                Vec::new()
            })
    } else {
        Vec::new()
    };
    let composed = compose_summary_context(&inherited, meeting_text);
    info!(
        "Summary context for meeting {}: use_inherited={}, inherited_levels={}, meeting_chars={}, composed_chars={}",
        meeting_id,
        use_inherited,
        inherited.len(),
        meeting_text.chars().count(),
        composed.chars().count()
    );
    composed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::folder::FolderRepository;
    use crate::database::repositories::meeting::MeetingsRepository;
    use crate::database::repositories::summary_context::test_util::{
        bare_pool, insert_meeting, test_pool,
    };
    use crate::database::repositories::summary_context::SummaryContextScope::{
        Folder, Global, Meeting,
    };

    fn level(scope: SummaryContextScope, label: Option<&str>, context: &str) -> InheritedContext {
        InheritedContext {
            scope,
            scope_id: String::new(),
            label: label.map(str::to_string),
            context: context.to_string(),
        }
    }

    #[test]
    fn no_inherited_levels_returns_the_meeting_text_byte_for_byte() {
        assert_eq!(compose_summary_context(&[], "  keep me \n"), "  keep me \n");
        assert_eq!(compose_summary_context(&[], ""), "");
    }

    #[test]
    fn inherited_levels_with_only_blank_text_change_nothing() {
        let blank = [level(Global, None, "  \n")];
        assert_eq!(compose_summary_context(&blank, "  keep me \n"), "  keep me \n");
    }

    #[test]
    fn global_only_with_an_empty_meeting_box_has_one_section_and_no_priority_line() {
        let inherited = [level(Global, None, "I am Noah, CTO at Acme.")];
        for meeting_text in ["", "   \n"] {
            assert_eq!(
                compose_summary_context(&inherited, meeting_text),
                "General context (applies to every meeting):\nI am Noah, CTO at Acme."
            );
        }
    }

    #[test]
    fn global_and_meeting_are_labelled_in_order_with_the_priority_line() {
        let inherited = [level(Global, None, " I am Noah. ")];
        assert_eq!(
            compose_summary_context(&inherited, " Renewal call. \n"),
            "General context (applies to every meeting):\nI am Noah.\n\n\
             Context for this meeting:\nRenewal call.\n\n\
             If these conflict, the more specific one takes priority: this meeting first, then the folder, then the general context."
        );
    }

    #[test]
    fn all_three_levels_keep_general_to_specific_order() {
        let inherited = [
            level(Global, None, "I am Noah."),
            level(Folder, Some("Vendor sync"), "Weekly sync with Initech."),
        ];
        let composed = compose_summary_context(&inherited, "Dana joined from legal.");
        let general = composed.find("General context (applies to every meeting):").unwrap();
        let folder = composed.find("Folder context (\"Vendor sync\"):\nWeekly sync with Initech.").unwrap();
        let meeting = composed.find("Context for this meeting:\nDana joined from legal.").unwrap();
        assert!(general < folder && folder < meeting);
        assert!(composed.ends_with(PRIORITY_NOTE));
    }

    #[test]
    fn folder_without_a_name_still_gets_a_heading() {
        let inherited = [level(Folder, None, "Weekly sync.")];
        assert_eq!(compose_summary_context(&inherited, ""), "Folder context:\nWeekly sync.");
    }

    #[tokio::test]
    async fn loads_the_global_level_only_when_it_has_text() {
        let pool = test_pool().await;
        assert!(load_inherited_contexts(&pool, "m1").await.unwrap().is_empty());

        SummaryContextRepository::save(&pool, Global, "", "I am Noah.", true).await.unwrap();
        SummaryContextRepository::save(&pool, Meeting, "m1", "meeting text", true).await.unwrap();
        assert_eq!(
            load_inherited_contexts(&pool, "m1").await.unwrap(),
            vec![level(Global, None, "I am Noah.")]
        );

        SummaryContextRepository::save(&pool, Global, "", "", true).await.unwrap();
        assert!(load_inherited_contexts(&pool, "m1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn resolve_composes_inherited_levels_unless_switched_off() {
        let pool = test_pool().await;
        SummaryContextRepository::save(&pool, Global, "", "I am Noah.", true).await.unwrap();

        assert_eq!(
            resolve_summary_context(&pool, "m1", " raw text ", false).await,
            " raw text "
        );
        let composed = resolve_summary_context(&pool, "m1", "raw text", true).await;
        assert!(composed.starts_with("General context (applies to every meeting):\nI am Noah."));
        assert!(composed.contains("Context for this meeting:\nraw text"));
    }

    #[tokio::test]
    async fn resolve_falls_back_to_the_meeting_text_when_the_table_cannot_be_read() {
        // No migrations: the summary_contexts table does not exist.
        let pool = bare_pool().await;
        assert_eq!(
            resolve_summary_context(&pool, "m1", "raw text", true).await,
            "raw text"
        );
    }

    /// A meeting filed in a new folder that has context. Returns the folder id.
    async fn filed_meeting(pool: &SqlitePool, meeting_id: &str, folder_name: &str, text: &str) -> String {
        insert_meeting(pool, meeting_id).await;
        let folder = FolderRepository::create(pool, folder_name).await.unwrap();
        MeetingsRepository::set_folder(pool, meeting_id, Some(&folder.id)).await.unwrap();
        SummaryContextRepository::save(pool, Folder, &folder.id, text, true).await.unwrap();
        folder.id
    }

    fn folder_level(folder_id: &str, name: &str, text: &str) -> InheritedContext {
        InheritedContext {
            scope: Folder,
            scope_id: folder_id.to_string(),
            label: Some(name.to_string()),
            context: text.to_string(),
        }
    }

    #[tokio::test]
    async fn a_filed_meeting_inherits_global_then_its_folder() {
        let pool = test_pool().await;
        SummaryContextRepository::save(&pool, Global, "", "I am Noah.", true).await.unwrap();
        let folder_id = filed_meeting(&pool, "m1", "Vendor sync", "Weekly sync with Initech.").await;

        assert_eq!(
            load_inherited_contexts(&pool, "m1").await.unwrap(),
            vec![
                level(Global, None, "I am Noah."),
                folder_level(&folder_id, "Vendor sync", "Weekly sync with Initech."),
            ]
        );
    }

    #[tokio::test]
    async fn an_unfiled_meeting_inherits_no_folder_level() {
        let pool = test_pool().await;
        SummaryContextRepository::save(&pool, Global, "", "I am Noah.", true).await.unwrap();
        filed_meeting(&pool, "filed", "Vendor sync", "Weekly sync.").await;
        insert_meeting(&pool, "unfiled").await;

        assert_eq!(
            load_inherited_contexts(&pool, "unfiled").await.unwrap(),
            vec![level(Global, None, "I am Noah.")]
        );
    }

    #[tokio::test]
    async fn a_folder_without_text_is_left_out() {
        let pool = test_pool().await;
        filed_meeting(&pool, "m1", "Vendor sync", "   ").await;
        assert!(load_inherited_contexts(&pool, "m1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_level_follows_the_meeting_to_another_folder_and_uses_the_current_name() {
        let pool = test_pool().await;
        let first = filed_meeting(&pool, "m1", "Vendor sync", "Weekly sync.").await;
        let second = FolderRepository::create(&pool, "Board").await.unwrap();
        SummaryContextRepository::save(&pool, Folder, &second.id, "Quarterly board prep.", true)
            .await
            .unwrap();

        FolderRepository::rename(&pool, &first, "Initech sync").await.unwrap();
        assert_eq!(
            load_inherited_contexts(&pool, "m1").await.unwrap(),
            vec![folder_level(&first, "Initech sync", "Weekly sync.")]
        );

        MeetingsRepository::set_folder(&pool, "m1", Some(&second.id)).await.unwrap();
        assert_eq!(
            load_inherited_contexts(&pool, "m1").await.unwrap(),
            vec![folder_level(&second.id, "Board", "Quarterly board prep.")]
        );
    }

    #[tokio::test]
    async fn a_deleted_folder_no_longer_applies() {
        let pool = test_pool().await;
        let folder_id = filed_meeting(&pool, "m1", "Vendor sync", "Weekly sync.").await;

        // A tombstoned folder is ignored even if a meeting still points at it.
        sqlx::query("UPDATE folders SET deleted_at = updated_at WHERE id = ?")
            .bind(&folder_id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(load_inherited_contexts(&pool, "m1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_a_folder_through_the_repository_stops_inheritance() {
        let pool = test_pool().await;
        let folder_id = filed_meeting(&pool, "m1", "Vendor sync", "Weekly sync.").await;
        FolderRepository::delete(&pool, &folder_id).await.unwrap();
        assert!(load_inherited_contexts(&pool, "m1").await.unwrap().is_empty());
    }
}
