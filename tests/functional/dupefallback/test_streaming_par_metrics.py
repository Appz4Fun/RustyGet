"""Framed callback measurement transport: no daemon or provider is mocked."""
import json
import unittest
import zlib
from streaming_par_test import callback_report, live_metric_reports

ROWS = [
    'ArticleCompleted bin {"version":1,"report_id":7,"index":0,"rows":2,"lower_ns":1000,"upper_ns":1003,"count":99}',
    'ArticleCompleted bin {"version":1,"report_id":7,"index":1,"rows":2,"lower_ns":999424,"upper_ns":1003519,"count":1}',
]
SUMMARY = 'ArticleCompleted metrics {"version":1,"report_id":7,"boundary":"queuecoord.article_completed","scope":"coordinator_lifetime","quiescent":true,"valid":true,"callbacks":100,"p99_lower_ns":1000,"p99_upper_ns":1003,"histogram_rows":2,"histogram_crc32":1222455617,"histogram_method":"loglinear128_ns_v1","transport":"log_records_v1"}'


def changed(line, **values):
    start = line.index('{')
    data = json.loads(line[start:])
    data.update(values)
    return line[:start] + json.dumps(data)


class CallbackTransportTest(unittest.TestCase):
    def test_complete_packet_preserves_counts_and_conservative_p99(self):
        report = callback_report('\n'.join(ROWS + [SUMMARY]), 100)
        self.assertEqual(report['histogram'], [[1000, 1003, 99], [999424, 1003519, 1]])
        self.assertEqual((report['p99_lower_ns'], report['p99_upper_ns']), (1000, 1003))
        self.assertEqual(report['callbacks'], 100)

    def test_missing_duplicate_and_foreign_rows_are_not_dropped(self):
        for rows in [ROWS[:1], [ROWS[0], ROWS[0]], ROWS + [ROWS[0]],
                     [changed(ROWS[0], report_id=8), ROWS[1]],
                     [changed(ROWS[0], index=2), ROWS[1]],
                     [changed(ROWS[0], rows=3), ROWS[1]]]:
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                callback_report('\n'.join(rows + [SUMMARY]), 1)

    def test_corrupt_crc_and_truncated_json_reject_the_packet(self):
        for lines in [ROWS + [changed(SUMMARY, histogram_crc32=1222455616)],
                      [ROWS[0][:-1], ROWS[1], SUMMARY]]:
            with self.subTest(lines=lines), self.assertRaises(ValueError):
                callback_report('\n'.join(lines), 1)

    def test_a_valid_crc_cannot_hide_inconsistent_callback_totals(self):
        # Independently frozen CRC32 of LE(7,101,2) and the two unchanged rows.
        summary = changed(SUMMARY, callbacks=101, histogram_crc32=1425713492,
                          p99_lower_ns=999424, p99_upper_ns=1003519)
        with self.assertRaises(ValueError):
            callback_report('\n'.join(ROWS + [summary]), 1)

    def test_summary_lifecycle_schema_and_p99_are_checked(self):
        for values in [{'valid': False}, {'quiescent': False}, {'version': 2},
                       {'boundary': 'copy_only'}, {'scope': 'partial'},
                       {'histogram_method': 'unknown'}, {'transport': 'unknown'},
                       {'histogram_rows': 1}, {'p99_lower_ns': 999424, 'p99_upper_ns': 1003519}]:
            with self.subTest(values=values), self.assertRaises(ValueError):
                callback_report('\n'.join(ROWS + [changed(SUMMARY, **values)]), 1)
        for summaries in [[], [SUMMARY, SUMMARY]]:
            with self.subTest(summaries=summaries), self.assertRaises(ValueError):
                callback_report('\n'.join(ROWS + summaries), 1)

    def test_insufficient_boundary_hits_are_not_zero_cost_success(self):
        with self.assertRaises(ValueError):
            callback_report('\n'.join(ROWS + [SUMMARY]), 101)




MAX_WIDTH_CRC = 3467611658

LIVE_KEYS = (
    'captured_articles captured_bytes capture_refusals cache_misses '
    'committed_replay_bytes ram_bytes_verified source_bytes_read backfill_bytes_read '
    'recovery_bytes_read target_verification_bytes_read tracked_exponents ready_recovery '
    'pending_folds verified_slices total_slices writer_wait_ns operation_wait_ns '
    'repair_attempts repair_completed_calls target_bytes_written payload_wait_ns '
    'slot_backfill_ns matrix_ns decode_ns target_write_ns target_sync_ns refold_ns '
    'target_verification_ns repair_call_ns'
).split()


def live_payload(event, **changes):
    maximum = (1 << 64) - 1
    payload = dict(event=event, nzb_id=-(1 << 31), generation=maximum)
    payload.update({key: maximum for key in LIVE_KEYS})
    payload.update(count_ready_observed=True, count_ready_to_handoff_ns=maximum,
                   decode_includes_writes=True, refold_includes_queued_completion=False,
                   logical_io=True)
    payload.update(changes)
    return payload


def live_frames(payload, report_id):
    raw = ('Live PAR metrics ' + json.dumps(payload, separators=(',', ':'))).encode()
    checksum = zlib.crc32(raw)
    if payload == live_payload('repair_complete'):
        assert len(raw) == 1428 and checksum == MAX_WIDTH_CRC
    chunks = [raw[offset:offset + 256] for offset in range(0, len(raw), 256)]
    return ['Live PAR frame ' + json.dumps(dict(
        version=1, nzb_id=-(1 << 31), generation=(1 << 64) - 1,
        report_id=report_id, index=index, parts=len(chunks), crc32=checksum,
        hex=chunk.hex()), separators=(',', ':'))
        for index, chunk in enumerate(chunks)]


def live_log(groups):
    return '\n'.join(line for group in groups for line in group) + '\n'


class LiveMetricTransportTest(unittest.TestCase):
    def setUp(self):
        self.groups = [live_frames(live_payload(event), index)
                       for index, event in enumerate(('started', 'repair_complete', 'disposed'), 1)]

    def test_maximum_counters_survive_complete_native_transport(self):
        records = live_metric_reports(live_log(self.groups))
        self.assertEqual([record['event'] for record in records], ['started', 'repair_complete', 'disposed'])
        self.assertEqual(records[1]['source_bytes_read'], (1 << 64) - 1)
        self.assertEqual(records[1]['count_ready_to_handoff_ns'], (1 << 64) - 1)
        self.assertTrue(all(len(line.encode()) + len('DETAIL\t') <= 1023
                            for group in self.groups for line in group))

    def test_incomplete_duplicate_foreign_or_invalid_parts_reject(self):
        report = self.groups[1]
        bad_groups = [report[:-1], report + [report[0]],
                      [changed(report[0], index=len(report))] + report[1:],
                      [changed(report[0], report_id=8)] + report[1:],
                      [changed(report[0], generation=7)] + report[1:],
                      [changed(report[0], parts=len(report) + 1)] + report[1:],
                      [changed(report[0], version=2)] + report[1:],
                      [changed(report[0], crc32=0)] + report[1:],
                      [changed(report[0], hex='not-hex')] + report[1:],
                      [report[0][:-1]] + report[1:]]
        for group in bad_groups:
            with self.subTest(group=group), self.assertRaises(ValueError):
                live_metric_reports(live_log([self.groups[0], group, self.groups[2]]))

    def test_missing_whole_report_or_disposal_footer_rejects(self):
        for groups in [self.groups[:2], [self.groups[0], self.groups[2]],
                       self.groups[1:], [self.groups[0], self.groups[0], self.groups[2]]]:
            with self.subTest(groups=groups), self.assertRaises(ValueError):
                live_metric_reports(live_log(groups))

    def test_crc_valid_invalid_inner_schema_cannot_fabricate_zero_metrics(self):
        invalid = [live_payload('unknown'), live_payload('repair_complete', source_bytes_read=True),
                   live_payload('repair_complete', source_bytes_read=-1),
                   live_payload('repair_complete', source_bytes_read=1 << 64),
                   live_payload('repair_complete', logical_io=False),
                   live_payload('repair_complete', count_ready_observed=False),
                   live_payload('repair_complete', nzb_id=4),
                   live_payload('repair_complete', generation=7)]
        missing = live_payload('repair_complete')
        del missing['source_bytes_read']
        invalid.append(missing)
        for payload in invalid:
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                live_metric_reports(live_log([self.groups[0], live_frames(payload, 2), self.groups[2]]))

    def test_extreme_report_sequence_is_rejected_without_allocating_the_gap(self):
        footer = [changed(line, report_id=(1 << 64) - 1) for line in self.groups[2]]
        with self.assertRaises(ValueError):
            live_metric_reports(live_log([self.groups[0], self.groups[1], footer]))

    def test_runtime_polling_never_accepts_incomplete_report_as_final(self):
        partial = live_log([self.groups[0], self.groups[1][:-1]])
        self.assertEqual([row['event'] for row in live_metric_reports(partial, strict=False)], ['started'])
        with self.assertRaises(ValueError):
            live_metric_reports(partial)
        cut_line = live_log([self.groups[0]]) + self.groups[1][0][:100]
        self.assertEqual(len(live_metric_reports(cut_line, strict=False)), 1)
        with self.assertRaises(ValueError):
            live_metric_reports(cut_line)


if __name__ == '__main__':
    unittest.main()
