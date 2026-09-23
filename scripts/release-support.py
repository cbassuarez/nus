#!/usr/bin/env python3
"""Offline maintenance admission gate. Dates are deliberate release decisions,
not guessed from tags; patches cannot extend the overlap. No network or writes.
"""
from datetime import datetime, timedelta, timezone
import re

def utc(value):
    date = datetime.fromisoformat(value.replace('Z', '+00:00'))
    if date.utcoffset() != timedelta(0):
        raise ValueError('Support dates must use UTC')
    return date

def evaluate(policy, tag, now=None):
    now = now or datetime.now(timezone.utc)
    if policy.get('schema') != 1 or policy.get('transition_days') != 30:
        raise ValueError('Unsupported release support policy')
    match = re.fullmatch(r'v(\d+)\.(\d+)\.(\d+)(-preview\.\d+)?', tag)
    if not match:
        raise ValueError('Invalid release version')
    if match[4]:
        return {'kind': 'preview', 'latest': False, 'support_until': None}
    lines = policy['feature_lines']
    prior_date = None
    prior_line = None
    for line in lines:
        if not re.fullmatch(r'\d+\.\d+', line['line']):
            raise ValueError('Invalid feature line')
        number = tuple(map(int, line['line'].split('.')))
        date = utc(line['promoted_at'])
        if prior_line is not None and (number <= prior_line or date < prior_date + timedelta(days=30)):
            raise ValueError('Feature lines must increase and allow the full 30-day transition')
        if date > now:
            raise ValueError('A stable feature line cannot be promoted in the future')
        prior_line, prior_date = number, date
    wanted = f'{int(match[1])}.{int(match[2])}'
    if not lines:
        raise ValueError('Register the first stable feature line and its promotion date before publishing')
    if wanted == lines[-1]['line']:
        return {'kind': 'current', 'latest': True, 'support_until': None}
    if len(lines) > 1 and wanted == lines[-2]['line']:
        until = utc(lines[-1]['promoted_at']) + timedelta(days=30)
        if now < until:
            return {'kind': 'previous', 'latest': False, 'support_until': until.isoformat()}
    raise ValueError('This feature line is outside the maintained release window')
