// C ABI of the Rust parts of nzbget (rust/src/ffi.rs)
#ifndef NZBGET_RS_H
#define NZBGET_RS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct
{
	char* data; // Rust-owned, NUL-terminated; never pass to free()/realloc()
	size_t len;
	size_t cap;
} NzbgetRsBuf;

// Release each result exactly once, with all fields unchanged. A zero buffer
// is also accepted. Copy the bytes before freeing if they must outlive the result.
void nzbget_rs_free(NzbgetRsBuf buf);

#include "nzbget_tls_cipher.h"

// FileSystem's path texts (rust/src/paths.rs): op 0 MakeValidFilename (flag:
// allow slashes), 1 SanitizePathSegment, 2 SanitizeRelativePath,
// 3 EscapePathForShell. Positions: op 0 BaseFileName's start, 1 the last
// '/' or '\\' (SIZE_MAX for none), 2 ExtractFilePathFromCmd's length.
// str is NULL (empty regardless of len) or readable for len bytes in one
// allocation, with len <= PTRDIFF_MAX. No input terminator is required.
// Text results own their storage; release exactly once with nzbget_rs_free.
// Panics/allocation failures abort; Rust never unwinds across this ABI.
NzbgetRsBuf nzbget_rs_path_text(int op, const char* str, size_t len, int flag);
size_t nzbget_rs_path_position(int op, const char* str, size_t len);

// Util text helpers (rust/src/util.rs). split_command_line: the words, each
// NUL-terminated, back to back (len covers all). trim_line: TrimRight(char*)
// (rightOnly) or Trim(char*) in place, returning the start. trim_string:
// TrimLeft/TrimRight/Trim of a std::string, or SanitizeLine (which blanks
// control characters in place): the kept range [*start, result). ends_with:
// EndsWith on byte ranges. parse_rfc822_date_time: WebUtil's, 0 if invalid.
// NULL inputs are empty; trim_line(NULL) returns NULL. trim_string accepts a
// NULL start output; rightSpace classifies unsigned bytes as the caller's char
// in its current C locale (NULL means no right whitespace). The callback must
// not throw or access the buffer. All input buffers remain caller-owned.
// Rust panics abort and never unwind across the ABI.
// Free returned buffers with nzbget_rs_free.

// FileSystem's file operations (rust/src/fsops.rs, POSIX). Buffers returned by
// these functions are Rust-owned and must be freed with nzbget_rs_free, never
// free/delete. Panics abort; they do not unwind across the ABI.
// NULL paths return 0.
int nzbget_rs_file_exists(const char* path);
int nzbget_rs_directory_exists(const char* path);

// ---- dupesearch (daemon/dupesearch/) ----
// DonorScore (rust/src/donorscore.rs): Base, Target (twin 0/1) and Rerank;

// ============================================================================
// Daemon Lifecycle Coordination C ABI (Requirement R2, Milestone M5)
// ============================================================================

// C++ subsystem bridge functions exported to Rust
void nzbget_c_util_init(void);
int  nzbget_c_nserv_main(int argc, char* argv[]);
int  nzbget_c_phase1_init(bool reloading);
int  nzbget_c_phase3_server_activate(void);
void nzbget_c_phase4_shutdown(bool reloading);
bool nzbget_c_has_active_unpack(void);
bool nzbget_c_is_reloading(void);
void nzbget_c_stop_server(bool reloading);
void nzbget_c_reload(void);
void nzbget_c_exit_proc(void);

// ============================================================================
// DupeArticleFallback, ParDamage, DupeProbe, ServerPool, ArticleFetcher &
// ArticleDownloader Hot-Path C ABI (rust/src/dupefallback.rs)
// ============================================================================


typedef struct {
    int64_t offset;
    int64_t size;
} NzbgetRsByteRange;

// ============================================================================
// DupeSearch, Fleet, DonorHealth, Newznab, FeedCoordinator, FeedInfo &
// FeedFile Hot-Path FFI (`rust/src/dupesearch_nzb.rs` / `rust/src/feed.rs`)
// ============================================================================

// ============================================================================
// StatMeter, WebProcessor, UrlCoordinator, DirectRenamer, NzbFile,
// QueueScript, PostScript, ScanScript, ScriptConfig & ScriptController FFI
// (`rust/src/statmeter.rs` / `rust/src/extension.rs`)
// ============================================================================



typedef intptr_t (*NzbgetRsConnHostFn)(
    void* ctx, int op, int a1, int a2, uintptr_t p1,
    const char* s1, const char* s2, const char* s3, char* buf, int* out_int);

typedef struct NzbgetRsNservEnv {
    void* ctx;
    const char* version;
    void (*write_stdout_cb)(const char* data, size_t len);
    void (*log_cb)(void* ctx, int level, const char* msg);
    void (*runtime_cb)(void* ctx, int op, int arg, const char* const* opts, size_t opt_len);
    uintptr_t (*conn_cb)(uintptr_t conn, int op, int a1, int a2, const char* s1, const char* s2, char* out_buf);
    NzbgetRsConnHostFn tls_host_cb;
    int debug_on;
} NzbgetRsNservEnv;
int nzbget_rs_nserv_run(int argc, const char* const* argv, const NzbgetRsNservEnv* env);

/* Consolidated WebDownloader & StatMeter FFI (`webdownload.rs`, `statmeter.rs`) */

// DownloadInfo & DiskState C ABI (rust/src/downloadinfo.rs, rust/src/diskstatepp.rs)



typedef struct {
    int64_t remaining_size;
    int64_t forced_size;
    int64_t article_cache;
    const char* memory_pressure;
    int effective_article_cache_mb;
    int effective_par_buffer_mb;
    int64_t download_rate;
    int download_limit;
    int thread_count;
    int post_job_count;
    int url_count;
    int up_time_sec;
    int download_time_sec;
    int64_t all_bytes;
    int64_t month_bytes;
    int64_t day_bytes;
    int download_paused;
    int server_stand_by;
    int post_paused;
    int scan_paused;
    int quota_reached;
    int64_t free_disk_space;
    int64_t total_disk_space;
    int inter_dir_empty;
    int64_t free_inter_disk_space;
    int64_t total_inter_disk_space;
    int server_time;
    int resume_time;
    int feed_active;
    int queued_scripts;
} NzbgetRsRpcStatusInput;

typedef struct {
    int id;
    int active;
} NzbgetRsRpcServerStatus;

typedef struct {
    int id;
    int kind;
    int time;
    const char* text;
} NzbgetRsRpcLogEntry;

typedef struct {
    int id;
    int64_t file_size;
    int64_t remaining_size;
    int post_time;
    int filename_confirmed;
    int paused;
    int nzb_id;
    const char* nzb_name;
    const char* nzb_filename;
    const char* subject;
    const char* filename;
    const char* dest_dir;
    const char* category;
    int priority;
    int active_downloads;
    int progress;
} NzbgetRsRpcFileItem;

typedef struct {
    int id;
    int kind;
    const char* name;
    const char* url;
    const char* filename;
    const char* dest_dir;
    const char* final_dir;
    const char* category;
    int par_status;
    int unpack_status;
    int move_status;
    int script_status;
    int delete_status;
    int mark_status;
    int url_status;
    int64_t size;
    int file_count;
    int min_time;
    int max_time;
    int total_articles;
    int current_success_articles;
    int current_failed_articles;
    int dupe_recovered_articles;
    int64_t dupe_recovered_bytes;
    int dupe_recovered_holes;
    int live_repairing;
    int health;
    int critical_health;
    const char* dupe_key;
    int dupe_score;
    int dupe_mode;
    int64_t downloaded_size;
    int download_sec;
    int post_total_sec;
    int has_post_start_time;
    int64_t cur_time;
    int64_t post_start_time;
    int par_sec;
    int repair_sec;
    int unpack_sec;
    int message_count;
    int cached_message_count;
    int extra_par_blocks;
} NzbgetRsRpcNzbInfoInput;

typedef struct {
    const char* name;
    const char* value;
} NzbgetRsRpcParamItem;

typedef struct {
    const char* name;
    int status;
} NzbgetRsRpcScriptItem;

typedef struct {
    int server_id;
    int success_articles;
    int failed_articles;
} NzbgetRsRpcServerStatItem;

typedef struct {
    const char* name;
    const char* value;
    int restricted;
} NzbgetRsRpcConfigItem;

typedef struct {
    const char* title;
    const char* filename;
    const char* url;
    int64_t size;
    int size_mb;
    const char* category;
    const char* add_category;
    int pause_nzb;
    int priority;
    int time;
    int match_status;
    int match_rule;
    const char* dupe_key;
    int dupe_score;
    int dupe_mode;
    int status;
} NzbgetRsRpcFeedItem;

typedef struct {
    const char* name;
    int is_url;
    const char* data;
    size_t data_len;
} NzbgetRsRpcFleetReqMember;

typedef struct NzbgetRsOptions NzbgetRsOptions;
typedef struct NzbgetRsLog NzbgetRsLog;
typedef struct NzbgetRsWorkState NzbgetRsWorkState;
typedef struct NzbgetRsServerPool NzbgetRsServerPool;

typedef uintptr_t (*NzbgetRsQueueSubsysHostFn)(
    void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
    const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);

void nzbget_rs_build_env_init(
    int flags, const char* version, const char* ssl_lib_name, const char* ssl_raw_version,
    NzbgetRsConnHostFn tls_host_cb,
    void (*write_stdout_cb)(const char* ptr, size_t len, int do_flush),
    NzbgetRsQueueSubsysHostFn queue_host_cb);
int nzbget_rs_thread_count_op(int op);
int nzbget_rs_tls_suppressed(uintptr_t slot);

NzbgetRsWorkState* nzbget_rs_workstate_new(uintptr_t notify_ctx, void (*notify_cb)(uintptr_t));
void nzbget_rs_workstate_free(NzbgetRsWorkState* ws);
int64_t nzbget_rs_workstate_ctl(const NzbgetRsWorkState* ws, int id, int write, int64_t val);

NzbgetRsServerPool* nzbget_rs_serverpool_new(void);
void nzbget_rs_serverpool_free(NzbgetRsServerPool* pool);
int nzbget_rs_serverpool_probe_tls(
    const NzbgetTlsServer* server, const char* cipher, int expected_ver,
    const char* numeric, uintptr_t deadline_ptr);

/* Extension & Script Subsystem End-to-End C ABI (`rust/src/extension.rs`, `rust/src/script.rs`, `rust/src/options.rs`) */

typedef struct NzbgetRsExtManager NzbgetRsExtManager;
NzbgetRsExtManager* nzbget_rs_ext_mgr_new(void);
void nzbget_rs_ext_mgr_free(NzbgetRsExtManager* mgr);

typedef struct NzbgetRsQueueScriptCoord NzbgetRsQueueScriptCoord;
NzbgetRsQueueScriptCoord* nzbget_rs_queue_script_new(void);
void nzbget_rs_queue_script_free(NzbgetRsQueueScriptCoord* h);

NzbgetRsBuf nzbget_rs_alloc_buf(const char* data, size_t len);

typedef struct NzbgetRsParsedOptions {
    int32_t download_rate;
    int32_t article_timeout;
    int32_t article_read_chunk_size;
    int32_t url_timeout;
    int32_t remote_timeout;
    int32_t article_retries;
    int32_t article_interval;
    int32_t url_retries;
    int32_t url_interval;
    int32_t control_port;
    int32_t secure_port;
    int32_t url_connections;
    int32_t log_buffer;
    int32_t rotate_log;
    int32_t umask;
    int32_t update_interval;
    int32_t write_buffer;
    int32_t nzb_dir_interval;
    int32_t nzb_dir_file_age;
    int32_t nzb_dir_archive_scan;
    int32_t nzb_dir_archive_action;
    int32_t disk_space;
    int32_t par_time_limit;
    int32_t keep_history;
    int32_t feed_history;
    int32_t time_correction;
    int32_t propagation_delay;
    int32_t article_cache;
    int32_t memory_pressure_interval;
    int32_t event_interval;
    int32_t par_buffer;
    int32_t par_threads;
    int32_t monthly_quota;
    int32_t quota_start_day;
    int32_t daily_quota;
    int32_t system_health_check;
    int32_t nzb_log;
    int32_t append_category_dir;
    int32_t continue_partial;
    int32_t flush_queue;
    int32_t dupe_check;
    int32_t par_repair;
    int32_t par_quick;
    int32_t par_rename;
    int32_t rar_rename;
    int32_t direct_rename;
    int32_t hard_linking;
    int32_t curses_nzb_name;
    int32_t curses_time;
    int32_t curses_group;
    int32_t crc_check;
    int32_t direct_write;
    int32_t raw_article;
    int32_t skip_write;
    int32_t crash_trace;
    int32_t crash_dump;
    int32_t par_pause_queue;
    int32_t script_pause_queue;
    int32_t nzb_cleanup_disk;
    int32_t form_auth;
    int32_t json_p;
    int32_t secure_control;
    int32_t unpack;
    int32_t direct_unpack;
    int32_t use_temp_unpack_dir;
    int32_t unpack_cleanup_disk;
    int32_t unpack_pause_queue;
    int32_t unpack_mutex_enforced;
    int32_t url_force;
    int32_t cert_check;
    int32_t reorder_files;
    int32_t post_download_rename;
    int32_t rename_after_unpack;
    int32_t output_mode;
    int32_t par_check;
    int32_t par_scan;
    int32_t dupe_article_fallback;
    int32_t dupe_stream_decompress;
    int32_t dupe_stream_timeout;
    int32_t dupe_search;
    int32_t dupe_search_dry_run;
    int32_t dupe_search_delay;
    int32_t dupe_search_max_donors;
    int32_t dupe_health_percent;
    int32_t dupe_health_min;
    int32_t dupe_health_max;
    int32_t dupe_body_checks;
    int32_t dupe_health_budget;
    int32_t dupe_min_alive;
    int32_t dupe_fast_donors;
    int32_t post_strategy;
    int32_t file_naming;
    int32_t health_check;
    int32_t info_target;
    int32_t warning_target;
    int32_t error_target;
    int32_t debug_target;
    int32_t detail_target;
    int32_t write_log;
    int32_t tls;
} NzbgetRsParsedOptions;

typedef void (*NzbgetRsOptionsHostFn)(
    void* ctx, int op, const int* ints, const char* const* strs, size_t count);
NzbgetRsOptions* nzbget_rs_options_new(
    const char* exe_name, const char* config_filename, int no_config, int no_disk_access,
    const char* const* cmd_opts, size_t cmd_opts_len, int flags, const char* version,
    void* cb_ctx, NzbgetRsOptionsHostFn cb_fn,
    NzbgetRsParsedOptions* out_scalars, int* out_state_flags);
void nzbget_rs_options_free(NzbgetRsOptions* handle);
const char* nzbget_rs_options_str(const NzbgetRsOptions* handle, int id);
int nzbget_rs_options_query(
    const NzbgetRsOptions* handle, int op, const char* s1, int a1,
    int* out_int, const char** out_strs, uintptr_t ctx,
    void (*cb)(uintptr_t, const char*, const char*, int));

NzbgetRsLog* nzbget_rs_log_new(int debug_build);
void nzbget_rs_log_free(NzbgetRsLog* log);
void nzbget_rs_log_emit(
    const NzbgetRsLog* log, int kind, const NzbgetRsOptions* options, const char* text);

uint32_t nzbget_rs_crc32_append(const uint8_t* block, uint32_t len, uint32_t init);

NzbgetRsBuf nzbget_rs_nzb_format_name(
    int op, const char* s1, const char* s2, const char* s3,
    const char* s4, const char* s5, int flag, int nzb_id);
int nzbget_rs_nzb_calc_health(
    int is_critical, int64_t size, int64_t par_size,
    int64_t current_failed_size, int64_t par_current_failed_size, int allow_estimation);
int nzbget_rs_nzb_is_dupe_success(
    int mark_status, int delete_status, int par_status, int unpack_status,
    int url_status, int health, int critical_health_est, int critical_health_exact,
    int health_check_dupe);
const char* nzbget_rs_nzb_make_text_status(
    int kind, int mark_status, int delete_status, int par_status, int unpack_status,
    int move_status, int url_status, int script_status, int health, int critical_health);
int nzbget_rs_nzb_completed_same_filename(const char* a, const char* b);
void nzbget_rs_nzb_calc_active_downloads_sec(
    int old_active, int new_active, int is_nzb_kind, int64_t cur_time,
    int* inout_download_sec, int64_t* inout_download_start_time,
    int* inout_download_start_sec, int* out_changed);
void nzbget_rs_nzb_update_completed_stats(
    int64_t file_success_size, int64_t file_failed_size,
    int file_failed_articles, int file_success_articles,
    int file_extra_priority, int file_par_file, int file_paused,
    int64_t* inout_success_size, int64_t* inout_failed_size,
    int* inout_failed_articles, int* inout_success_articles,
    int* inout_extra_priority, int64_t* inout_par_success_size,
    int64_t* inout_par_failed_size, int* inout_remaining_par_count,
    int* inout_paused_file_count);
void nzbget_rs_nzb_update_deleted_stats(
    int64_t file_size, int64_t file_success_size, int64_t file_failed_size,
    int64_t file_missed_size, int file_total_articles, int file_success_articles,
    int file_failed_articles, int file_missed_articles, int64_t file_remaining_size,
    int file_extra_priority, int file_par_file, int file_paused,
    int* inout_file_count, int64_t* inout_size, int64_t* inout_cur_success_size,
    int64_t* inout_failed_size, int* inout_failed_articles, int64_t* inout_cur_failed_size,
    int* inout_total_articles, int* inout_cur_success_articles, int* inout_cur_failed_articles,
    int64_t* inout_remaining_size, int* inout_extra_priority, int* inout_remaining_par_count,
    int64_t* inout_par_size, int64_t* inout_par_cur_success_size, int64_t* inout_par_failed_size,
    int64_t* inout_par_cur_failed_size, int* inout_paused_file_count, int64_t* inout_paused_size);

void nzbget_rs_util_format_time(int64_t time_sec, char* buffer, int bufsize);

typedef struct RsDownloadCurrentStatsAcc {
    int64_t remaining_size;
    int64_t current_success_size;
    int64_t current_failed_size;
    int64_t paused_size;
    int64_t par_current_success_size;
    int64_t par_current_failed_size;
    int32_t current_success_articles;
    int32_t current_failed_articles;
    int32_t extra_priority;
    int32_t paused_file_count;
    int32_t remaining_par_count;
    int32_t _pad;
} RsDownloadCurrentStatsAcc;

void nzbget_rs_nzb_update_current_stats_step(
    RsDownloadCurrentStatsAcc* acc, int64_t file_remaining_size,
    int file_success_articles, int file_failed_articles,
    int64_t file_success_size, int64_t file_failed_size,
    int file_extra_priority, int file_paused, int file_par_file);
void nzbget_rs_nzb_calc_min_max_time(
    const int64_t* times, size_t count, int64_t* out_min, int64_t* out_max);
void nzbget_rs_server_stat_apply_op(
    int stat_op, int delta_success, int delta_failed,
    int* inout_success, int* inout_failed);
int nzbget_rs_script_calc_total_status(const int* statuses, size_t count);

int nzbget_rs_diskstate_op(int op, uintptr_t p1, int a1, const char* s1);
int nzbget_rs_socket_deadline_wait(
    int fd, int read, int write, void* ctx, int64_t (*rem_cb)(void*));

typedef struct NzbgetRsDaemonEnv {
    void* ctx; int a1; int a2; int a3; int a4; int a5; int a6;
    const char* s1; const char* s2; const char* s3; const char* s4; const char* s5;
    void (*segv_cb)(void*, const char*); void (*global_log_cb)(int, const char*); int (*reload_cb)(int);
    void (*log_cb)(void*, int, const char*); int (*action_cb)(void*, int, int, const char*, const char*);
} NzbgetRsDaemonEnv;
int nzbget_rs_daemon_exec(int op, const NzbgetRsDaemonEnv* env);
typedef uintptr_t (*NzbgetRsRepairCtrlHostFn)(
    void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
    const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);
typedef struct NzbgetRsParChecker NzbgetRsParChecker;
NzbgetRsParChecker* nzbget_rs_par_checker_new(void);
void nzbget_rs_par_checker_free(NzbgetRsParChecker* ctrl);
int nzbget_rs_par_checker_exec(
    const NzbgetRsParChecker* ctrl, int op, int a1, const char* s1,
    NzbgetRsRepairCtrlHostFn host_cb, void* ctx);
typedef struct NzbgetRsDirectUnpack NzbgetRsDirectUnpack;
NzbgetRsDirectUnpack* nzbget_rs_direct_unpack_new(int nzb_id, uintptr_t thread_ptr);
void nzbget_rs_direct_unpack_free(NzbgetRsDirectUnpack* ctrl);
int nzbget_rs_direct_unpack_exec(
    const NzbgetRsDirectUnpack* ctrl, int op, uintptr_t p1, uintptr_t p2);
typedef struct NzbgetRsPostJob NzbgetRsPostJob;
NzbgetRsPostJob* nzbget_rs_post_job_new(int kind, uintptr_t thread_ptr, uintptr_t post_ptr, int nzb_id);
void nzbget_rs_post_job_free(NzbgetRsPostJob* job);
int nzbget_rs_post_job_op(const NzbgetRsPostJob* job, int op, int a1, uintptr_t p1);

typedef int (*NzbgetRsXmlRpcActionFn)(
    void* ctx, int op, int a1, int a2, int a3, int a4,
    const char* s1, const char* s2, const char* s3, const char* s4,
    const char* body, size_t body_len,
    const int32_t* ids, size_t ids_len,
    const NzbgetRsRpcParamItem* params, size_t params_len,
    uint32_t* out_u32, double* out_f64, NzbgetRsBuf* out_buf);

typedef void (*NzbgetRsXmlRpcEmitFn)(
    void* emit_ctx, int sub,
    int a1, int a2, int a3, int a4, int a5, int a6,
    int64_t i1, int64_t i2, int64_t i3,
    const char* s1, const char* s2, const char* s3, const char* s4,
    const void* p1, size_t n1, const void* p2, size_t n2,
    const void* p3, size_t n3, const void* p4, size_t n4);

typedef int (*NzbgetRsXmlRpcCollectFn)(
    void* ctx, int kind, int a1, int a2, int a3, int a4, int a5,
    const char* s1, const char* s2, const char* s3,
    const char* s4, const char* s5, const char* s6,
    const void* p1, size_t n1,
    void* emit_ctx, NzbgetRsXmlRpcEmitFn emit_cb);

int nzbget_rs_script_config_exec(
    int op, const char* p1, size_t n1, const char* p2, size_t n2,
    void* ctx, int (*cancel_cb)(void*), NzbgetRsBuf* out_buf);

typedef void (*NzbgetRsBinRpcListPushNzbFn)(
    void* b_ctx, uint32_t id, uint32_t kind, int64_t size, int64_t rem_size, int64_t paused_size,
    uint32_t paused_files, uint32_t rem_pars, uint32_t priority,
    const char* filename, const char* name, const char* dest_dir,
    const char* category, const char* queued_filename);
typedef void (*NzbgetRsBinRpcListPushParamFn)(
    void* b_ctx, uint32_t owner_idx, const char* name, const char* value);
typedef void (*NzbgetRsBinRpcListPushFileFn)(
    void* b_ctx, uint32_t id, uint32_t nzb_idx, int64_t size, int64_t rem_size,
    uint32_t paused, uint32_t fn_confirmed, uint32_t active_dl,
    const char* nzb_name, const char* subject, const char* filename);
typedef void (*NzbgetRsBinRpcPostPushFn)(
    void* enc_ctx, uint32_t id, uint32_t stage, uint32_t stage_prog, uint32_t file_prog,
    uint32_t start_sec, uint32_t stage_sec, const char* filename, const char* name,
    const char* dest_dir, const char* label);
typedef void (*NzbgetRsBinRpcHistPushFn)(
    void* enc_ctx, uint32_t id, uint32_t kind, uint32_t time, int64_t size,
    uint32_t file_cnt, uint32_t par_status, uint32_t script_status,
    uint32_t url_status, const char* name);

typedef struct NzbgetRsRpcQueueStatus {
    int post_job_count;
    int url_count;
    int queued_scripts;
    int64_t remaining_size;
    int64_t forced_size;
} NzbgetRsRpcQueueStatus;

typedef struct NzbgetRsRpcHost {
    NzbgetRsXmlRpcActionFn rpc_action_cb;
    NzbgetRsXmlRpcCollectFn rpc_collect_cb;
    int (*binrpc_action_cb)(
        int kind, int a1, int a2, int a3, int a4,
        const char* s1, const char* s2, const char* s3,
        const char* body, int body_len,
        const int32_t* ids, size_t ids_len,
        const char* const* names, size_t names_len);
    void (*binrpc_list_cb)(
        int want_files, void* b_ctx,
        NzbgetRsBinRpcListPushNzbFn push_nzb,
        NzbgetRsBinRpcListPushParamFn push_param,
        NzbgetRsBinRpcListPushFileFn push_file,
        int* out_post_jobs, int64_t* out_rem_size);
    void (*binrpc_post_cb)(void* enc_ctx, NzbgetRsBinRpcPostPushFn push_post);
    void (*binrpc_history_cb)(int show_hidden, void* enc_ctx, NzbgetRsBinRpcHistPushFn push_hist);
} NzbgetRsRpcHost;

void nzbget_rs_remote_srv_ctl(int op, const NzbgetRsRpcHost* host);

typedef struct NzbgetRsQueueCoordinator NzbgetRsQueueCoordinator;
typedef struct NzbgetRsPrePostProcessor NzbgetRsPrePostProcessor;
NzbgetRsQueueCoordinator* nzbget_rs_queue_coord_new(void);
void nzbget_rs_queue_coord_free(NzbgetRsQueueCoordinator* ptr);
uintptr_t nzbget_rs_queue_coord_exec(
    const NzbgetRsQueueCoordinator* ptr, int op, int a1, int a2,
    uintptr_t p1, uintptr_t p2, uintptr_t p3, const char* s1, const int* ints,
    NzbgetRsQueueSubsysHostFn host_cb, void* ctx);
NzbgetRsPrePostProcessor* nzbget_rs_prepost_new(uintptr_t owner);
void nzbget_rs_prepost_free(NzbgetRsPrePostProcessor* ptr);
uintptr_t nzbget_rs_prepost_exec(
    const NzbgetRsPrePostProcessor* ptr, int op, int a1, int a2,
    uintptr_t p1, uintptr_t p2, uintptr_t p3, const char* s1,
    void* host_ctx, NzbgetRsQueueSubsysHostFn host_cb);
typedef struct NzbgetRsArticleDownloader NzbgetRsArticleDownloader;
NzbgetRsArticleDownloader* nzbget_rs_article_dl_new(
    int has_analyzer, int parcheck_enabled, uintptr_t initial_conn, const char* info_name);
void nzbget_rs_article_dl_free(NzbgetRsArticleDownloader* ptr);
uintptr_t nzbget_rs_article_dl_exec(
    NzbgetRsArticleDownloader* ptr, int op, int a1, int a2,
    uintptr_t p1, uintptr_t p2, const char* s1,
    NzbgetRsQueueSubsysHostFn host_cb, void* ctx);

#ifdef __cplusplus
}
#endif

#endif
