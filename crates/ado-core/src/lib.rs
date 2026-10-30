//! Pure domain logic for ADO Planner. No I/O.

pub mod dates;
pub mod error;
pub mod flavor;
pub mod form_layout;
pub mod hierarchy;
pub mod html_field;
pub mod organization_url;
pub mod overlays;
pub mod planner_prefs;
pub mod root_types;
pub mod team_members;
pub mod types;

pub use dates::{
    DEFAULT_BAR_DAYS, GanttBarDisplay, GanttBarKind, IterationHint, build_add_dependency_patch,
    build_clear_dates_patch, build_dates_patch, build_field_patch, build_start_target_patch,
    client_rollup, dates_column_text, from_gantt_exclusive_end, gantt_bar_display, is_unscheduled,
    iso_date_only, iteration_hint_for_path, parse_iso_date, schedule_from_iteration,
    to_gantt_inclusive_target, type_has_start_and_target,
};
pub use error::Error;
pub use flavor::{
    FLAVORS, Flavor, KNOWN_TYPE_COLORS, color_for_type, default_flavor, is_dark_flavor,
};
pub use form_layout::{
    FieldMetadata, LayoutControl, LayoutGroup, LayoutPage, ProcessLayout, all_editable_controls,
    build_form_patch, draft_from_form_values, flatten_layout, form_field_name, kind_from_metadata,
    missing_required_fields, reference_name_from_form_field, strip_mnemonic, validate_form_values,
    work_item_form_defaults,
};
pub use hierarchy::{
    AreaPathValue, BatchWorkItem, HIERARCHY_FIELDS, assemble_forest, build_hierarchy_wiql,
    build_untyped_hierarchy_wiql, chunk_ids, escape_wiql, identity_from_field, iso_date_from_field,
    map_batch_work_item, parent_id_from_fields, predecessor_ids,
};
pub use html_field::{html_equivalent, html_to_markdown, is_plain_html_field, markdown_to_html};
pub use organization_url::shorten_organization_url;
pub use overlays::apply_overlays;
pub use planner_prefs::{
    PlannerPrefs, choose_available, parse_planner_prefs, serialize_planner_prefs,
};
pub use root_types::{BacklogLevel, resolve_root_types, top_backlog_types_from_levels};
pub use team_members::{TeamMemberIdentityRow, map_team_member_identities};
pub use types::*;
