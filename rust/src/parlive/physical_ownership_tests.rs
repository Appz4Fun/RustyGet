//! Actual destructive-call regressions: labels alone must never authorize writes.
use super::*;
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};

static NEXT_JOB: AtomicI32 = AtomicI32::new(-1_961_000);
struct Fixture {
    root: PathBuf,
    job: i32,
}
impl Fixture {
    fn new() -> Self {
        let job = NEXT_JOB.fetch_sub(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rustyget-physical-{}-{}-é",
            std::process::id(),
            job
        ));
        std::fs::create_dir(&root).unwrap();
        Self { root, job }
    }
    fn context(&self, name: &[u8]) -> SourceWriteContext {
        source_write_context(self.job, 40, &self.root, name).unwrap()
    }
    fn seal(&self, path: &Path) -> std::sync::Arc<WriteCoordinator> {
        self.context(b"part40.rar")
            .attest_existing_source()
            .unwrap();
        let gate = download_writes(self.job, &self.root).unwrap();
        let generation = SessionGeneration(1);
        gate.advance_generation(generation).unwrap();
        gate.begin_repair(
            generation,
            vec![WriterTarget {
                file_id: 40,
                path: path.to_owned(),
            }],
            reserve_control(self.job, 4096, 500 << 20).unwrap(),
        )
        .unwrap()
        .publish(&[(40, 0)], || ())
        .unwrap();
        gate
    }
    fn finish(self) {
        forget_download_writes(self.job);
        std::fs::remove_dir_all(self.root).unwrap();
    }
}

fn assert_preserved(path: &Path, before: &std::fs::Metadata, bytes: &[u8]) {
    let after = std::fs::metadata(path).unwrap();
    let actual = std::fs::read(path).unwrap();
    println!(
        "physical before=({}, {}, {}) after=({}, {}, {}) bytes={actual:?}",
        before.dev(),
        before.ino(),
        before.len(),
        after.dev(),
        after.ino(),
        after.len()
    );
    assert_eq!(actual, bytes, "refusal must precede deletion/truncation");
    assert_eq!(
        (after.dev(), after.ino(), after.len()),
        (before.dev(), before.ino(), before.len())
    );
}

#[test]
fn actual_output_path_must_match_its_source_context_before_preallocation() {
    let f = Fixture::new();
    let path = f.root.join("other.rar");
    std::fs::write(&path, b"other original").unwrap();
    let before = std::fs::metadata(&path).unwrap();
    let context = f.context(b"part40.rar");
    let gate = download_writes(f.job, &f.root).unwrap();
    gate.advance_generation(SessionGeneration(1)).unwrap();
    let result = crate::artwritercore::article_writer_create_output_file_guarded(
        path.as_os_str().as_bytes(),
        4096,
        &context,
    );
    assert_preserved(&path, &before, b"other original");
    assert!(result.is_err());
    f.finish();
}

#[test]
fn symlink_output_must_be_refused_before_removing_the_link() {
    let f = Fixture::new();
    let real = f.root.join("real.rar");
    let link = f.root.join("part40.rar");
    std::fs::write(&real, b"real original").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let before = std::fs::symlink_metadata(&link).unwrap();
    let context = f.context(b"part40.rar");
    let result = crate::artwritercore::article_writer_create_output_file_guarded(
        link.as_os_str().as_bytes(),
        4096,
        &context,
    );
    let after = std::fs::symlink_metadata(&link).unwrap();
    println!(
        "symlink inode before={} after={} length={}",
        before.ino(),
        after.ino(),
        after.len()
    );
    assert!(
        after.file_type().is_symlink(),
        "must not remove the alias before attesting"
    );
    assert_eq!(after.ino(), before.ino());
    assert_eq!(std::fs::read(real).unwrap(), b"real original");
    assert!(result.is_err());
    f.finish();
}

#[test]
fn intermediate_root_alias_cannot_bypass_a_certified_target() {
    let f = Fixture::new();
    let path = f.root.join("part40.rar");
    std::fs::write(&path, b"certified bytes").unwrap();
    let _original = f.context(b"part40.rar");
    let _gate = f.seal(&path);
    let alias = f.root.with_extension("alias");
    std::os::unix::fs::symlink(&f.root, &alias).unwrap();
    let alias_job = NEXT_JOB.fetch_sub(1, Ordering::Relaxed);
    let alias_context = source_write_context(alias_job, 40, &alias, b"part40.rar").unwrap();
    let before = std::fs::metadata(&path).unwrap();
    let result = crate::artwritercore::article_writer_create_output_file_guarded(
        alias.join("part40.rar").as_os_str().as_bytes(),
        4096,
        &alias_context,
    );
    assert_preserved(&path, &before, b"certified bytes");
    assert!(result.is_err());
    forget_download_writes(alias_job);
    std::fs::remove_file(alias).unwrap();
    f.finish();
}

#[test]
fn live_hardlinked_source_is_refused_before_replacement() {
    let f = Fixture::new();
    let path = f.root.join("part40.rar");
    let other = f.root.join("other.rar");
    std::fs::write(&path, b"shared original").unwrap();
    std::fs::hard_link(&path, &other).unwrap();
    let before = std::fs::metadata(&path).unwrap();
    let context = f.context(b"part40.rar");
    let gate = download_writes(f.job, &f.root).unwrap();
    gate.advance_generation(SessionGeneration(1)).unwrap();
    let result = crate::artwritercore::article_writer_create_output_file_guarded(
        path.as_os_str().as_bytes(),
        4096,
        &context,
    );
    assert_preserved(&path, &before, b"shared original");
    assert_eq!(std::fs::metadata(other).unwrap().ino(), before.ino());
    assert!(result.is_err());
    f.finish();
}

#[test]
fn ordinary_output_in_unicode_root_and_unprotected_hardlink_still_work() {
    let f = Fixture::new();
    let path = f.root.join("part40.rar");
    let context = f.context(b"part40.rar");
    crate::artwritercore::article_writer_create_output_file_guarded(
        path.as_os_str().as_bytes(),
        8,
        &context,
    )
    .unwrap();
    assert_eq!(std::fs::metadata(path).unwrap().len(), 8);
    let original = f.root.join("ordinary");
    let link = f.root.join("ordinary-link");
    std::fs::write(&original, b"ordinary").unwrap();
    std::fs::hard_link(&original, &link).unwrap();
    crate::artwritercore::article_writer_create_output_file(link.as_os_str().as_bytes(), 4)
        .unwrap();
    assert_eq!(std::fs::read(original).unwrap(), b"ordinary");
    assert_eq!(std::fs::metadata(link).unwrap().len(), 4);
    f.finish();
}

#[test]
fn own_generated_output_is_allowed_but_another_owner_is_preserved() {
    let f = Fixture::new();
    let own = f.root.join("40.out.tmp");
    let other = f.root.join("41.out.tmp");
    std::fs::write(&other, b"other owner").unwrap();
    let context = f.context(b"part40.rar");
    crate::artwritercore::article_writer_create_output_file_guarded(
        own.as_os_str().as_bytes(), 8, &context).unwrap();
    assert_eq!(std::fs::metadata(own).unwrap().len(), 8);
    assert!(crate::artwritercore::article_writer_create_output_file_guarded(
        other.as_os_str().as_bytes(), 4096, &context).is_err());
    assert_eq!(std::fs::read(other).unwrap(), b"other owner");
    f.finish();
}

#[test]
fn article_temporary_has_exact_owner_number_and_independent_physical_gate() {
    use std::io::Write;
    let source = Fixture::new();
    let scratch = Fixture::new();
    let own = scratch.root.join("40.007.tmp");
    let other = scratch.root.join("41.007.tmp");
    std::fs::write(&other, b"foreign article").unwrap();
    let context = source.context(b"part40.rar")
        .with_article_temporary(&scratch.root, 7).unwrap();
    let (mut file, context, permit) = context.open_attested(&own, true).unwrap();
    file.write_all(b"same frame").unwrap();
    drop(permit);
    drop(context.descriptor_permit(&file).unwrap());
    assert_eq!(std::fs::read(&own).unwrap(), b"same frame");
    assert!(context.open_attested(&other, true).is_err());
    assert!(context.open_attested(&scratch.root.join("40.008.tmp"), true).is_err());
    assert_eq!(std::fs::read(other).unwrap(), b"foreign article");
    drop(file);
    drop(context);
    source.finish();
    scratch.finish();
}

#[test]
fn own_output_of_a_file_named_into_a_subfolder_is_in_the_root() {
    use std::io::Write;
    let f = Fixture::new();
    // DirectRename named the file in progress "Sub/dr.bin"; its data is still in the
    // root's "<id>.out.tmp".
    let context = f.context(b"Sub/dr.bin");
    let (mut file, context, permit) = context
        .open_attested(&f.root.join("40.out.tmp"), true)
        .unwrap();
    file.write_all(b"data").unwrap();
    drop(permit);
    assert_eq!(std::fs::read(f.root.join("40.out.tmp")).unwrap(), b"data");
    assert!(context
        .open_attested(&f.root.join("Sub").join("40.out.tmp"), true)
        .is_err());
    drop(file);
    drop(context);
    f.finish();
}

#[test]
fn scratch_alias_cannot_replace_a_different_cohorts_certified_input() {
    let source = Fixture::new();
    let destination = Fixture::new();
    let path = destination.root.join("40.007.tmp");
    std::fs::write(&path, b"certified source").unwrap();
    let _gate = destination.seal(&path);
    let context = source.context(b"part40.rar")
        .with_article_temporary(&destination.root, 7).unwrap();
    let before = std::fs::metadata(&path).unwrap();
    assert!(context.open_attested(&path, true).is_err());
    assert_preserved(&path, &before, b"certified source");
    drop(context);
    source.finish();
    destination.finish();
}

#[test]
fn alternate_root_cannot_reopen_a_closed_cohort_with_an_inflight_writer() {
    let f = Fixture::new();
    let path = f.root.join("part40.rar");
    std::fs::write(&path, b"inflight bytes").unwrap();
    let context = f.context(b"part40.rar");
    context.attest_existing_source().unwrap();
    let writer = context.permit().unwrap();
    forget_download_writes(f.job);
    let alias = f.root.with_extension("closed-alias");
    std::os::unix::fs::symlink(&f.root, &alias).unwrap();
    let other_job = NEXT_JOB.fetch_sub(1, Ordering::Relaxed);
    let other = source_write_context(other_job, 40, &alias, b"part40.rar").unwrap();
    let attempt = other.open_attested(&alias.join("part40.rar"), true);
    if let Ok((file, _, permit)) = &attempt {
        file.set_len(0).unwrap();
        let _ = permit;
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"inflight bytes");
    assert!(attempt.is_err());
    drop(writer);
    forget_download_writes(other_job);
    std::fs::remove_file(alias).unwrap();
    f.finish();
}

#[test]
fn different_job_cannot_write_a_live_source_through_a_root_alias() {
    let f = Fixture::new();
    let path = f.root.join("part40.rar");
    std::fs::write(&path, b"live source").unwrap();
    let context = f.context(b"part40.rar");
    context.attest_existing_source().unwrap();
    let gate = download_writes(f.job, &f.root).unwrap();
    gate.advance_generation(SessionGeneration(1)).unwrap();
    let alias = f.root.with_extension("other-job");
    std::os::unix::fs::symlink(&f.root, &alias).unwrap();
    let other_job = NEXT_JOB.fetch_sub(1, Ordering::Relaxed);
    let other = source_write_context(other_job, 41, &alias, b"part40.rar").unwrap();
    let attempt = other.open_attested(&alias.join("part40.rar"), true);
    if let Ok((file, _, permit)) = &attempt {
        file.set_len(0).unwrap();
        let _ = permit;
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"live source");
    assert!(attempt.is_err());
    forget_download_writes(other_job);
    std::fs::remove_file(alias).unwrap();
    f.finish();
}

#[test]
fn rejected_job_cleanup_keeps_the_other_jobs_certified_gate_alive() {
    let f = Fixture::new();
    let path = f.root.join("part40.rar");
    std::fs::write(&path, b"verified").unwrap();
    let gate = f.seal(&path);
    let other_job = NEXT_JOB.fetch_sub(1, Ordering::Relaxed);
    let other = source_write_context(other_job, 41, &f.root, b"part40.rar").unwrap();
    assert!(other.open_attested(&path, true).is_err());
    forget_download_writes(other_job);
    assert!(gate.is_current_generation(SessionGeneration(1)));
    assert_eq!(std::fs::read(path).unwrap(), b"verified");
    f.finish();
}

#[test]
fn scratch_writer_cannot_replace_another_jobs_unsealed_live_source() {
    let source = Fixture::new();
    let destination = Fixture::new();
    let path = destination.root.join("40.007.tmp");
    std::fs::write(&path, b"other live source").unwrap();
    let destination_context = destination.context(b"40.007.tmp");
    destination_context.attest_existing_source().unwrap();
    let gate = download_writes(destination.job, &destination.root).unwrap();
    gate.advance_generation(SessionGeneration(1)).unwrap();
    let context = source.context(b"part40.rar")
        .with_article_temporary(&destination.root, 7).unwrap();
    let attempt = context.open_attested(&path, true);
    if let Ok((file, _, permit)) = &attempt {
        file.set_len(0).unwrap();
        let _ = permit;
    }
    assert_eq!(std::fs::read(path).unwrap(), b"other live source");
    assert!(attempt.is_err());
    assert!(!gate.is_current_generation(SessionGeneration(1)));
    source.finish();
    destination.finish();
}

#[test]
fn exhausted_live_budget_does_not_bypass_a_foreign_certified_source() {
    let f = Fixture::new();
    let path = f.root.join("part40.rar");
    std::fs::write(&path, b"certified").unwrap();
    let gate = f.seal(&path);
    let limit = crate::parchecker::effective_live_pool_bytes();
    assert!(limit > 0);
    // Drain a pool of this test's own: holding all of the shared one starves parallel tests.
    let pool = Arc::new(BudgetPool::with_size(limit));
    crate::parchecker::TEST_BUDGET_POOL.with(|p| *p.borrow_mut() = Some(Arc::clone(&pool)));
    let mut held = Vec::new();
    let mut chunk = 1024 * 1024u64;
    while chunk != 0 {
        while let Some(lease) = pool.acquire(chunk, limit) { held.push(lease); }
        chunk /= 2;
    }
    let other_job = NEXT_JOB.fetch_sub(1, Ordering::Relaxed);
    let other = source_write_context(other_job, 41, &f.root, b"part40.rar").unwrap();
    let attempt = other.open_attested(&path, true);
    if let Ok((file, _, permit)) = &attempt {
        file.set_len(0).unwrap();
        let _ = permit;
    }
    assert!(attempt.is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"certified");
    assert!(gate.is_current_generation(SessionGeneration(1)));
    drop(held);
    crate::parchecker::TEST_BUDGET_POOL.with(|p| *p.borrow_mut() = None);
    forget_download_writes(other_job);
    f.finish();
}

#[test]
fn preindex_cross_job_writer_conflict_cannot_disappear_at_live_startup() {
    let f = Fixture::new();
    let path = f.root.join("part40.rar");
    std::fs::write(&path, b"preindex source").unwrap();
    let context = f.context(b"part40.rar");
    context.attest_existing_source().unwrap();
    let writer = context.permit().unwrap();
    let gate = download_writes(f.job, &f.root).unwrap();
    let alias = f.root.with_extension("preindex-alias");
    std::os::unix::fs::symlink(&f.root, &alias).unwrap();
    let other_job = NEXT_JOB.fetch_sub(1, Ordering::Relaxed);
    let other = source_write_context(other_job, 41, &alias, b"part40.rar").unwrap();
    let attempt = other.open_attested(&alias.join("part40.rar"), true);
    if let Ok((file, _, permit)) = &attempt {
        file.set_len(0).unwrap();
        let _ = permit;
    }
    assert!(attempt.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"preindex source");
    drop(writer);
    assert!(gate.advance_generation(SessionGeneration(1)).is_err());
    forget_download_writes(other_job);
    std::fs::remove_file(alias).unwrap();
    f.finish();
}

#[test]
fn preindex_cross_job_alias_may_write_an_unrelated_file() {
    let f = Fixture::new();
    let path = f.root.join("part40.rar");
    std::fs::write(&path, b"preindex source").unwrap();
    let context = f.context(b"part40.rar");
    context.attest_existing_source().unwrap();
    let writer = context.permit().unwrap();
    let gate = download_writes(f.job, &f.root).unwrap();
    let alias = f.root.with_extension("unrelated-alias");
    std::os::unix::fs::symlink(&f.root, &alias).unwrap();
    let other_job = NEXT_JOB.fetch_sub(1, Ordering::Relaxed);
    let other = source_write_context(other_job, 41, &alias, b"other.rar").unwrap();
    let attempt = other.open_attested(&alias.join("other.rar"), true);
    assert!(attempt.is_ok(), "an unrelated file was refused");
    drop(attempt);
    assert_eq!(std::fs::read(&path).unwrap(), b"preindex source");
    drop(writer);
    assert!(gate.advance_generation(SessionGeneration(1)).is_ok(), "an unrelated write poisoned live startup");
    forget_download_writes(other_job);
    std::fs::remove_file(alias).unwrap();
    f.finish();
}
