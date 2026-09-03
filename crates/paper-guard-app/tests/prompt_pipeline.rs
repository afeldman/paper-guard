//! End-to-end regression tests for the external prompt system at the shared
//! pipeline level:
//!
//! * a run without any external prompt files uses the embedded defaults and
//!   records `embedded-default` provenance with a stable SHA-256;
//! * an external prompt file changes the prompt used *without a rebuild* and
//!   is recorded as `external` with a different hash;
//! * a deliberately broken external prompt fails the run (no silent fallback);
//! * the reviewed paper file is never modified.

use std::path::Path;

use paper_guard_app::config::AppConfig;
use paper_guard_app::pipeline::run_pipeline;

fn sample_paper() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("sample-paper.tex")
}

fn config_with_prompt_dir(dir: Option<&Path>) -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.llm.provider = "mock".into();
    cfg.prompts.directory = dir.map(|d| d.to_string_lossy().into_owned());
    cfg
}

#[tokio::test]
async fn default_run_uses_embedded_prompts_with_stable_hashes() {
    let paper = sample_paper();
    let original = std::fs::read(&paper).unwrap();

    let run_a = {
        let data_dir = tempfile::tempdir().unwrap();
        let cfg = config_with_prompt_dir(None);
        run_pipeline(
            &paper.to_string_lossy(),
            &cfg,
            data_dir.path().to_str().unwrap(),
            None,
            false,
        )
        .await
        .unwrap()
    };
    let run_b = {
        let data_dir = tempfile::tempdir().unwrap();
        let cfg = config_with_prompt_dir(None);
        run_pipeline(
            &paper.to_string_lossy(),
            &cfg,
            data_dir.path().to_str().unwrap(),
            None,
            false,
        )
        .await
        .unwrap()
    };

    // All five enabled reviewers record embedded-default provenance.
    let usage_a: Vec<_> = run_a
        .run
        .reviewer_results
        .iter()
        .filter_map(|o| o.prompt_usage.clone())
        .collect();
    assert_eq!(usage_a.len(), 5);
    for u in &usage_a {
        assert_eq!(u.source, "embedded-default");
        assert!(u.path.is_none());
        assert_eq!(u.sha256.len(), 64, "sha256 must be 64 hex chars");
    }
    // Same config + same paper => identical prompt hashes across runs.
    let hash_of = |usage: &Vec<paper_guard_ledger::PromptUsage>, role: &str| {
        usage
            .iter()
            .find(|u| u.prompt == role)
            .unwrap()
            .sha256
            .clone()
    };
    let usage_b: Vec<_> = run_b
        .run
        .reviewer_results
        .iter()
        .filter_map(|o| o.prompt_usage.clone())
        .collect();
    assert_eq!(
        hash_of(&usage_a, "scientific"),
        hash_of(&usage_b, "scientific")
    );
    assert_eq!(hash_of(&usage_a, "figures"), hash_of(&usage_b, "figures"));

    // The paper file itself is untouched by the run.
    assert_eq!(std::fs::read(&paper).unwrap(), original);
}

#[tokio::test]
async fn external_prompt_is_used_without_rebuild_and_recorded() {
    let paper = sample_paper();
    let prompt_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        prompt_dir.path().join("scientific.md"),
        "CUSTOM: focus on reproducibility only.",
    )
    .unwrap();

    let data_dir = tempfile::tempdir().unwrap();
    let cfg = config_with_prompt_dir(Some(prompt_dir.path()));
    let run = run_pipeline(
        &paper.to_string_lossy(),
        &cfg,
        data_dir.path().to_str().unwrap(),
        None,
        false,
    )
    .await
    .unwrap();

    let usage: Vec<_> = run
        .run
        .reviewer_results
        .iter()
        .filter_map(|o| o.prompt_usage.clone())
        .collect();
    assert_eq!(usage.len(), 5);

    let sci = usage.iter().find(|u| u.prompt == "scientific").unwrap();
    assert_eq!(sci.source, "external");
    assert!(sci.path.as_deref().unwrap().contains("scientific.md"));
    let others_embedded = usage
        .iter()
        .filter(|u| u.prompt != "scientific")
        .all(|u| u.source == "embedded-default");
    assert!(
        others_embedded,
        "only scientific.md exists, so only it is external"
    );

    // The embedded-default hash for scientific differs from the external one.
    let cfg_default = config_with_prompt_dir(None);
    let data_dir_default = tempfile::tempdir().unwrap();
    let run_default = run_pipeline(
        &paper.to_string_lossy(),
        &cfg_default,
        data_dir_default.path().to_str().unwrap(),
        None,
        false,
    )
    .await
    .unwrap();
    let embedded_sci = run_default
        .run
        .reviewer_results
        .iter()
        .filter_map(|o| o.prompt_usage.clone())
        .find(|u| u.prompt == "scientific")
        .unwrap();
    assert_ne!(sci.sha256, embedded_sci.sha256);
}

#[cfg(unix)]
#[tokio::test]
async fn broken_external_prompt_fails_run_without_fallback() {
    use std::os::unix::fs::PermissionsExt;
    let paper = sample_paper();
    let prompt_dir = tempfile::tempdir().unwrap();
    let bad = prompt_dir.path().join("adversarial.md");
    std::fs::write(&bad, "unreadable prompt").unwrap();
    std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o000)).unwrap();

    let data_dir = tempfile::tempdir().unwrap();
    let cfg = config_with_prompt_dir(Some(prompt_dir.path()));
    let err = match run_pipeline(
        &paper.to_string_lossy(),
        &cfg,
        data_dir.path().to_str().unwrap(),
        None,
        false,
    )
    .await
    {
        Ok(_) => panic!("a broken external prompt must fail the run"),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("cannot read external prompt file"),
        "broken external prompt must fail loudly: {err}"
    );
    std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o600)).unwrap();
}
