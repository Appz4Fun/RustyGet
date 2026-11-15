"""Framed callback measurement transport: no daemon or provider is mocked."""
import json
import unittest
from streaming_par_test import callback_report

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


if __name__ == '__main__':
    unittest.main()
