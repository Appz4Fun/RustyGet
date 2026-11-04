//! StreamRepair, ContentMap, ParChecker, and RepairController hot-path helpers (Batch 42).

/// Returns whether a `NewsServer` is active at primary normalized level (`0`).
pub fn src_stream_repair_is_active_primary_server(
    active: bool,
    norm_level: i32,
) -> bool {
    active && norm_level == 0
}

/// Returns whether `ProveDonorOnIntactFile` has valid non-empty set keys and a selected stand-in file.
pub fn src_stream_repair_can_prove_donor_on_intact_file(
    target_set_empty: bool,
    donor_set_empty: bool,
    has_stand_in: bool,
) -> bool {
    !target_set_empty && !donor_set_empty && has_stand_in
}

/// Returns whether `ProveDonorOnIntactFile` should stop iterating candidate intact member files.
pub fn src_stream_repair_should_stop_intact_proof_loop(
    is_stopped: bool,
    tried: i32,
    max_intact_proof_files: i32,
) -> bool {
    is_stopped || tried >= max_intact_proof_files
}

/// Returns whether a donor member's volume-key match belongs in the current intact-proof pass (`0` = same number, `1` = other).
pub fn src_stream_repair_matches_intact_candidate_pass(
    same_number: bool,
    pass: i32,
) -> bool {
    same_number == (pass == 0)
}

/// Returns whether `RepairWholeFile` should skip a size-probe article index before checking the `probed` set.
pub fn src_stream_repair_should_skip_size_probe_index(
    sized: bool,
    is_stopped: bool,
    index: usize,
    articles_len: usize,
) -> bool {
    sized || is_stopped || index >= articles_len
}

/// Returns whether `MarkRepaired` should transition a `CompletedFile` to `cfSuccess`.
pub fn src_stream_repair_should_mark_completed_repaired(
    file_id: i32,
    target_file_id: i32,
    status: i32,
    cf_success: i32,
) -> bool {
    file_id == target_file_id && status != cf_success
}

/// Returns whether `MarkRecreated` should transition a `CompletedFile` from `cfFailure` to `cfPartial`.
pub fn src_stream_repair_should_mark_completed_recreated(
    file_id: i32,
    target_file_id: i32,
    status: i32,
    cf_failure: i32,
) -> bool {
    file_id == target_file_id && status == cf_failure
}

/// Evaluates whether `VerifyDonor` should terminate after comparing a probe batch.
/// Returns `true` when `VerifyDonor` should return `*out_verified` immediately.
pub fn src_stream_repair_verify_donor_completed(
    is_stopped: bool,
    total_compared: i64,
    required_compare: i64,
    saw_varied_data: bool,
    out_verified: &mut bool,
) -> bool {
    if is_stopped || (total_compared >= required_compare && saw_varied_data) {
        *out_verified = !is_stopped;
        true
    } else {
        *out_verified = false;
        false
    }
}

/// Returns whether `ExecCrossPackRepair` can attempt M4 archive decompression for a donor set.
pub fn src_stream_repair_can_try_decompress_fallback(
    live_mode: bool,
    decompress_opt: bool,
    is_compressible: bool,
) -> bool {
    !live_mode && decompress_opt && is_compressible
}

/// Evaluates the per-window loop guard in `VerifyDonorSet`, `VerifyEncryptedFromSource`, and `VerifyDonorInnerFile`:
/// - `1`: stopped (`return false`)
/// - `2`: sufficient varied data compared (`break`)
/// - `0`: continue verifying current window
pub fn src_stream_repair_verify_window_loop_stop(
    is_stopped: bool,
    total_compared: i64,
    required: i64,
    saw_varied_data: bool,
) -> i32 {
    if is_stopped {
        1
    } else if total_compared >= required && saw_varied_data {
        2
    } else {
        0
    }
}

/// Returns the bounded patch chunk size `min(hole_end - pos, max_chunk)`.
pub fn src_stream_repair_calc_patch_chunk_size(
    hole_end: i64,
    pos: i64,
    max_chunk: i64,
) -> i64 {
    hole_end.saturating_sub(pos).min(max_chunk)
}

/// Returns whether a donor set member index is valid and has non-empty articles for `MaterializeDonorSet`.
pub fn src_stream_repair_is_member_valid_for_materialize(
    member_index: i32,
    files_len: usize,
    articles_empty: bool,
) -> bool {
    member_index >= 0 && (member_index as usize) < files_len && !articles_empty
}

/// Returns whether a materialized donor member has a valid size and no remaining holes.
pub fn src_stream_repair_is_materialized_member_complete(
    member_size: i64,
    holes_empty: bool,
) -> bool {
    member_size >= 0 && holes_empty
}

/// Classifies how an interrupted `StreamRepairJob` should be updated in `RepairCompleted`:
/// - `1`: target is fully repaired (`target_holes_empty && target_decoded_size > 0`) -> erase job
/// - `2`: retain job, setting `*out_update_job_size = job_decoded_size <= 0 && target_decoded_size > 0`
pub fn src_stream_repair_classify_interrupted_job_update(
    target_holes_empty: bool,
    target_decoded_size: i64,
    job_decoded_size: i64,
    out_update_job_size: &mut bool,
) -> i32 {
    if target_holes_empty && target_decoded_size > 0 {
        *out_update_job_size = false;
        1
    } else {
        *out_update_job_size = job_decoded_size <= 0 && target_decoded_size > 0;
        2
    }
}

/// Returns whether `ReadTargetCipher` failed to map a positive-sized cipher range.
pub fn src_content_map_is_empty_or_invalid_cipher_pieces(
    pieces_empty: bool,
    cipher_range_size: i64,
) -> bool {
    pieces_empty && cipher_range_size > 0
}

/// Returns whether a `ContentSource` exists and has a positive size for `ContentMapper` map builders.
pub fn src_content_map_can_build_bare_or_split_member(
    has_source: bool,
    source_size: i64,
) -> bool {
    has_source && source_size > 0
}

/// Returns whether a RAR inner file's data offset and packed size lie within the volume `ContentSource`.
pub fn src_content_map_rar_data_run_geometry_valid(
    data_offset: i64,
    packed_size: i64,
    has_source: bool,
    source_size: i64,
) -> bool {
    if data_offset < 0 || packed_size < 0 || !has_source {
        return false;
    }
    match data_offset.checked_add(packed_size) {
        Some(end) => end <= source_size,
        None => false,
    }
}

/// Returns whether a RAR volume run carries mappable plaintext data (`data_offset >= 0 && packed_size > 0`).
pub fn src_content_map_rar_can_emit_plain_run(
    data_offset: i64,
    packed_size: i64,
) -> bool {
    data_offset >= 0 && packed_size > 0
}

/// Returns whether `ParChecker` should continue requesting additional recovery blocks when repair is not yet possible.
pub fn src_parchecker_should_process_twin_missing_blocks(
    res: i32,
    e_repair_not_possible: i32,
    is_stopped: bool,
) -> bool {
    res == e_repair_not_possible && !is_stopped
}

/// Returns whether `RepairController::RequestMorePars` needs another `FindPars` pass (`block_found < block_needed`).
pub fn src_repair_ctrl_should_fetch_more_par_pass(
    block_found: i32,
    block_needed: i32,
) -> bool {
    block_found < block_needed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_repair_intact_and_whole_file_helpers() {
        assert!(src_stream_repair_is_active_primary_server(true, 0));
        assert!(!src_stream_repair_is_active_primary_server(false, 0));
        assert!(!src_stream_repair_is_active_primary_server(true, 1));

        assert!(src_stream_repair_can_prove_donor_on_intact_file(false, false, true));
        assert!(!src_stream_repair_can_prove_donor_on_intact_file(true, false, true));
        assert!(!src_stream_repair_can_prove_donor_on_intact_file(false, true, true));
        assert!(!src_stream_repair_can_prove_donor_on_intact_file(false, false, false));

        assert!(src_stream_repair_should_stop_intact_proof_loop(true, 0, 2));
        assert!(src_stream_repair_should_stop_intact_proof_loop(false, 2, 2));
        assert!(!src_stream_repair_should_stop_intact_proof_loop(false, 1, 2));

        assert!(src_stream_repair_matches_intact_candidate_pass(true, 0));
        assert!(!src_stream_repair_matches_intact_candidate_pass(false, 0));
        assert!(src_stream_repair_matches_intact_candidate_pass(false, 1));

        assert!(src_stream_repair_should_skip_size_probe_index(true, false, 0, 5));
        assert!(src_stream_repair_should_skip_size_probe_index(false, true, 0, 5));
        assert!(src_stream_repair_should_skip_size_probe_index(false, false, 5, 5));
        assert!(!src_stream_repair_should_skip_size_probe_index(false, false, 4, 5));
    }

    #[test]
    fn test_stream_repair_mark_and_verify_helpers() {
        assert!(src_stream_repair_should_mark_completed_repaired(7, 7, 1, 2));
        assert!(!src_stream_repair_should_mark_completed_repaired(7, 7, 2, 2));
        assert!(!src_stream_repair_should_mark_completed_repaired(6, 7, 1, 2));

        assert!(src_stream_repair_should_mark_completed_recreated(7, 7, 0, 0));
        assert!(!src_stream_repair_should_mark_completed_recreated(7, 7, 1, 0));

        let mut verified = false;
        assert!(src_stream_repair_verify_donor_completed(true, 100, 64, true, &mut verified));
        assert!(!verified);
        assert!(src_stream_repair_verify_donor_completed(false, 100, 64, true, &mut verified));
        assert!(verified);
        assert!(!src_stream_repair_verify_donor_completed(false, 100, 64, false, &mut verified));
        assert!(!verified);

        assert!(src_stream_repair_can_try_decompress_fallback(false, true, true));
        assert!(!src_stream_repair_can_try_decompress_fallback(true, true, true));
        assert!(!src_stream_repair_can_try_decompress_fallback(false, false, true));
        assert!(!src_stream_repair_can_try_decompress_fallback(false, true, false));

        assert_eq!(src_stream_repair_verify_window_loop_stop(true, 0, 64, false), 1);
        assert_eq!(src_stream_repair_verify_window_loop_stop(false, 64, 64, true), 2);
        assert_eq!(src_stream_repair_verify_window_loop_stop(false, 64, 64, false), 0);
        assert_eq!(src_stream_repair_verify_window_loop_stop(false, 32, 64, true), 0);
    }

    #[test]
    fn test_stream_repair_materialize_and_job_helpers() {
        assert_eq!(src_stream_repair_calc_patch_chunk_size(5000, 1000, 2048), 2048);
        assert_eq!(src_stream_repair_calc_patch_chunk_size(2500, 1000, 2048), 1500);

        assert!(src_stream_repair_is_member_valid_for_materialize(1, 3, false));
        assert!(!src_stream_repair_is_member_valid_for_materialize(-1, 3, false));
        assert!(!src_stream_repair_is_member_valid_for_materialize(3, 3, false));
        assert!(!src_stream_repair_is_member_valid_for_materialize(1, 3, true));

        assert!(src_stream_repair_is_materialized_member_complete(1024, true));
        assert!(!src_stream_repair_is_materialized_member_complete(-1, true));
        assert!(!src_stream_repair_is_materialized_member_complete(1024, false));

        let mut update_size = false;
        assert_eq!(
            src_stream_repair_classify_interrupted_job_update(true, 1024, 1024, &mut update_size),
            1
        );
        assert!(!update_size);
        assert_eq!(
            src_stream_repair_classify_interrupted_job_update(false, 1024, 0, &mut update_size),
            2
        );
        assert!(update_size);
        assert_eq!(
            src_stream_repair_classify_interrupted_job_update(false, 1024, 1024, &mut update_size),
            2
        );
        assert!(!update_size);
    }

    #[test]
    fn test_content_map_and_par_helpers() {
        assert!(src_content_map_is_empty_or_invalid_cipher_pieces(true, 16));
        assert!(!src_content_map_is_empty_or_invalid_cipher_pieces(false, 16));
        assert!(!src_content_map_is_empty_or_invalid_cipher_pieces(true, 0));

        assert!(src_content_map_can_build_bare_or_split_member(true, 4096));
        assert!(!src_content_map_can_build_bare_or_split_member(false, 4096));
        assert!(!src_content_map_can_build_bare_or_split_member(true, 0));

        assert!(src_content_map_rar_data_run_geometry_valid(64, 1000, true, 1064));
        assert!(!src_content_map_rar_data_run_geometry_valid(-1, 1000, true, 1064));
        assert!(!src_content_map_rar_data_run_geometry_valid(64, -1, true, 1064));
        assert!(!src_content_map_rar_data_run_geometry_valid(64, 1000, false, 1064));
        assert!(!src_content_map_rar_data_run_geometry_valid(64, 1001, true, 1064));

        assert!(src_content_map_rar_can_emit_plain_run(0, 512));
        assert!(!src_content_map_rar_can_emit_plain_run(-1, 512));
        assert!(!src_content_map_rar_can_emit_plain_run(0, 0));

        assert!(src_parchecker_should_process_twin_missing_blocks(2, 2, false));
        assert!(!src_parchecker_should_process_twin_missing_blocks(1, 2, false));
        assert!(!src_parchecker_should_process_twin_missing_blocks(2, 2, true));

        assert!(src_repair_ctrl_should_fetch_more_par_pass(3, 5));
        assert!(!src_repair_ctrl_should_fetch_more_par_pass(5, 5));
    }
}
