pub mod canonical;
pub mod crypto;
pub mod export;
pub mod index;
pub mod key_store;
pub mod limits;
pub mod sign;
pub mod types;
pub mod validate;

#[cfg(test)]
pub mod test_support;

pub use export::{export_evidence_bundle, preview_evidence_bundle, ExportRequest};
pub use index::{load_import_index, register_imported_bundle_id, save_import_index_atomic};
pub use sign::{sign_manifest, sign_manifest_with_provider, signing_payload};
pub use types::{
    DeviceIdentity, EvidenceExportResult, EvidenceManifest, EvidenceProposedTask,
    EvidenceTaskDraft, EvidenceValidationReport, SignerProvider, ValidationContext,
};
pub use validate::{
    assert_reviewed_before_task_create, create_task_draft, create_task_draft_with_selection,
    resolve_task_selection, validate_evidence_bundle,
};
