//! Network tests against huggingface.co. Run with
//! `cargo test -p sakshi-models -- --ignored`.

use sakshi_models::{Error, Hub, Manifest, Role};

#[test]
#[ignore = "network: queries huggingface.co"]
fn official_diarization_model_passes_custom_validation() {
    let manifest = Manifest::bundled().unwrap();
    let pinned = manifest.default_for(Role::Diarization).unwrap();

    let custom = Hub::new()
        .unwrap()
        .inspect_custom(&pinned.repo, &pinned.filename, Some(&pinned.revision))
        .unwrap();

    assert_eq!(custom.architecture, "sortformer");
    assert_eq!(custom.entry.role, Role::Diarization);
    assert_eq!(custom.entry.revision, pinned.revision);
    assert_eq!(custom.entry.size, pinned.size);
    assert_eq!(custom.entry.sha256, pinned.sha256);
}

#[test]
#[ignore = "network: queries huggingface.co"]
fn missing_file_is_reported() {
    let err = Hub::new()
        .unwrap()
        .inspect_custom("nvidia/Nemotron-3-Diarization", "nope.gguf", None)
        .unwrap_err();
    assert!(matches!(err, Error::FileNotFound { .. }), "{err}");
}
