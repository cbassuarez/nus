import importlib.util
from pathlib import Path
from datetime import datetime, timezone
import unittest

spec = importlib.util.spec_from_file_location('support', Path(__file__).parents[1] / 'release-support.py')
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)

class SupportTests(unittest.TestCase):
    def policy(self):
        return {'schema': 1, 'transition_days': 30, 'feature_lines': [
            {'line': '0.7', 'promoted_at': '2026-07-01T00:00:00Z'},
            {'line': '0.8', 'promoted_at': '2026-08-01T00:00:00Z'},
        ]}

    def test_current_previous_and_exact_deadline(self):
        now = datetime(2026, 8, 30, tzinfo=timezone.utc)
        self.assertTrue(support.evaluate(self.policy(), 'v0.8.2', now)['latest'])
        before = support.evaluate(self.policy(), 'v0.7.40', now)
        self.assertFalse(before['latest'])
        self.assertEqual(before['support_until'], '2026-08-31T00:00:00+00:00')
        with self.assertRaises(ValueError):
            support.evaluate(self.policy(), 'v0.7.41', datetime(2026, 8, 31, tzinfo=timezone.utc))

    def test_feature_promotions_cannot_shorten_overlap(self):
        policy = self.policy()
        policy['feature_lines'].append({'line': '0.9', 'promoted_at': '2026-08-15T00:00:00Z'})
        with self.assertRaisesRegex(ValueError, '30-day'):
            support.evaluate(policy, 'v0.9.0', datetime(2026, 9, 1, tzinfo=timezone.utc))

    def test_stable_requires_a_deliberate_first_promotion(self):
        policy = self.policy(); policy['feature_lines'] = []
        with self.assertRaisesRegex(ValueError, 'Register'):
            support.evaluate(policy, 'v0.1.0')
        self.assertEqual(support.evaluate(policy, 'v0.1.0-preview.1')['kind'], 'preview')

if __name__ == '__main__': unittest.main()
