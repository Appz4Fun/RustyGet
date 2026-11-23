/*
 * This file is part of RustyGet.
 *
 * This program is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

/*
 * Copies of one release added together are one fleet (docs/DROP_AS_FLEET.md).
 * The Add dialog groups its files by the name copies of a release share: the
 * daemon's normalized release name (rust/src/releasename.rs `normalize`) without
 * the suffix a download manager added (`_5`, ` (1)`, `.nzb.2.queued`, see
 * `strip_copy_suffix` there). This file mirrors those functions exactly; the
 * same cases run in tests/webui/dupe-group.js and in the Rust unit tests.
 */
var DupeGroup = (new function()
{
	'use strict';

	var MAX_NAME_LENGTH = 512;
	var GROUP_KEY_PREFIX = 'dupes:';
	var EXT_WORDS = ['nzb', 'mkv', 'mp4', 'm4v', 'avi', 'ts', 'rar', 'par2', '7z', 'zip', 'nfo',
		'sfv', 'srr', 'srt', 'sub', 'idx', 'jpg', 'png', 'txt'];
	var JUNK = ['xpost', 'postbot', 'obfuscated', 'scrambled', 'asrequested', 'rp', 'rakuv',
		'buymore', 'chamele0n', 'sample', 'repost'];

	function isDigit(c) { return c >= '0' && c <= '9'; }
	function isAlpha(c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
	function isSpace(c) { return c === ' ' || c === '\t' || c === '\n' || c === '\r' || c === '\v' || c === '\f'; }
	function lower(s) { return s.replace(/[A-Z]/g, function(c) { return c.toLowerCase(); }); }
	function eqIcase(text, pos, word) { return lower(text.substr(pos, word.length)) === word && text.length >= pos + word.length; }

	function trim(text, chars)
	{
		var b = 0, e = text.length;
		while (b < e && chars.indexOf(text[b]) > -1) b++;
		while (e > b && chars.indexOf(text[e - 1]) > -1) e--;
		return text.substring(b, e);
	}

	function digitsAt(s, pos)
	{
		var n = 0;
		while (pos + n < s.length && isDigit(s[pos + n])) n++;
		return n;
	}

	function trailingDigits(s)
	{
		var n = 0;
		while (n < s.length && isDigit(s[s.length - 1 - n])) n++;
		return n;
	}

	function endsIcase(s, suffix)
	{
		return s.length >= suffix.length && lower(s.substr(s.length - suffix.length)) === suffix;
	}

	// releasename.rs extension_tail
	function extensionTail(text, pos)
	{
		var rest = text.substr(pos);
		if (eqIcase(rest, 0, 'part'))
		{
			var n = digitsAt(rest, 4);
			if (n > 0 && rest.length === 4 + n + 4 && eqIcase(rest, 4 + n, '.rar')) return true;
		}
		if (eqIcase(rest, 0, 'vol'))
		{
			var v = digitsAt(rest, 3);
			if (v > 0 && rest[3 + v] === '+')
			{
				var m = digitsAt(rest, 4 + v);
				if (m > 0 && rest.length === 4 + v + m + 5 && eqIcase(rest, 4 + v + m, '.par2')) return true;
			}
		}
		if (rest.length === 6 && eqIcase(rest, 0, '7z.') && isDigit(rest[3]) && isDigit(rest[4]) && isDigit(rest[5])) return true;
		if (rest.length === 3 && (lower(rest[0]) === 'r' || lower(rest[0]) === 'z') && isDigit(rest[1]) && isDigit(rest[2])) return true;
		return EXT_WORDS.indexOf(lower(rest)) > -1;
	}

	function findExtension(text)
	{
		for (var i = 0; i < text.length; i++)
		{
			if (text[i] === '.' && extensionTail(text, i + 1)) return i;
		}
		return -1;
	}

	// releasename.rs junk_from
	function junkFrom(text, pos)
	{
		var reachable = [], seen = [], todo = [pos];
		while (todo.length)
		{
			var p = todo.pop();
			if (seen[p] || p >= text.length || '.-_ '.indexOf(text[p]) < 0) continue;
			seen[p] = true;
			var q = p + 1;
			for (var w = 0; w < JUNK.length; w++)
			{
				var word = JUNK[w];
				if (!eqIcase(text, q, word)) continue;
				var ends = [q + word.length];
				if (word === 'rakuv')
				{
					var e = q + word.length;
					while (e < text.length && (isAlpha(text[e]) || isDigit(text[e]) || text[e] === '_'))
					{
						e++;
						ends.push(e);
					}
				}
				for (var k = 0; k < ends.length; k++)
				{
					if (!reachable[ends[k]])
					{
						reachable[ends[k]] = true;
						todo.push(ends[k]);
					}
				}
			}
		}
		return !!reachable[text.length];
	}

	function findJunk(text)
	{
		for (var i = 0; i < text.length; i++)
		{
			if (junkFrom(text, i)) return i;
		}
		return -1;
	}

	// releasename.rs clean
	function clean(name)
	{
		var text = trim(String(name).substr(0, MAX_NAME_LENGTH), ' \t\r\n');
		for (;;)
		{
			var before = text;
			var pos = findExtension(text);
			var n = text.length;
			if (pos > -1)
			{
				text = text.substr(0, pos);
			}
			else if (n > 4 && text[n - 4] === '.' && isDigit(text[n - 3]) && isDigit(text[n - 2]) && isDigit(text[n - 1]) &&
				lower(text[n - 5]) !== 'h' && lower(text[n - 5]) !== 'x')
			{
				text = text.substr(0, n - 4);
			}
			if (text === before) break;
		}
		var stripped = '', depth = 0, closer = '';
		for (var i = 0; i < text.length; i++)
		{
			var ch = text[i];
			if (depth === 0 && (ch === '[' || ch === '{'))
			{
				depth = 1;
				closer = ch === '[' ? ']' : '}';
				stripped += ' ';
			}
			else if (depth === 1)
			{
				if (ch === closer) depth = 0;
			}
			else
			{
				stripped += ch;
			}
		}
		if (depth === 1) stripped = text;
		text = trim(stripped, ' \t\r\n');
		var junk = findJunk(text);
		if (junk > -1) text = text.substr(0, junk);
		return trim(text, ' ._-');
	}

	// releasename.rs normalize
	this.normalize = function(name)
	{
		var text = lower(clean(name));
		var out = '', separator = false;
		for (var i = 0; i < text.length; i++)
		{
			var ch = text[i];
			if (isSpace(ch) || '._-()+,\0'.indexOf(ch) > -1)
			{
				separator = true;
				continue;
			}
			if (separator && out.length) out += '.';
			separator = false;
			out += ch;
		}
		return out;
	};

	function endsInGroupTag(text)
	{
		var word = text;
		for (var i = text.length - 1; i >= 0; i--)
		{
			if (text[i] === '.' || isSpace(text[i]))
			{
				word = text.substr(i + 1);
				break;
			}
		}
		var dash = word.lastIndexOf('-');
		if (dash < 0) return false;
		var tail = word.substr(dash + 1);
		for (var k = 0; k < tail.length; k++)
		{
			if (isAlpha(tail[k])) return true;
		}
		return false;
	}

	// releasename.rs strip_copy_suffix
	this.stripCopySuffix = function(name)
	{
		var s = trim(String(name), ' \t\r\n');
		var before, d;
		for (;;)
		{
			before = s.length;
			if (endsIcase(s, '.queued')) s = s.substr(0, s.length - 7);
			d = trailingDigits(s);
			if (d > 0 && d <= 3 && s.length > d + 1 && s[s.length - d - 1] === '.')
			{
				var head = s.substr(0, s.length - d - 1);
				if (endsIcase(head, '.nzb')) s = head;
			}
			if (endsIcase(s, '.nzb')) s = s.substr(0, s.length - 4);
			if (s.length === before) break;
		}
		for (;;)
		{
			before = s.length;
			if (s[s.length - 1] === ')')
			{
				var inner = s.substr(0, s.length - 1);
				d = trailingDigits(inner);
				if (d > 0 && d <= 3 && inner.length > d && inner[inner.length - d - 1] === '(')
				{
					s = inner.substr(0, inner.length - d - 1);
					while (s.length && isSpace(s[s.length - 1])) s = s.substr(0, s.length - 1);
				}
			}
			d = trailingDigits(s);
			if (d > 0 && d <= 3 && s.length > d + 1)
			{
				var restLen = s.length - d - 1;
				if ('_-.'.indexOf(s[restLen]) > -1 && endsInGroupTag(s.substr(0, restLen))) s = s.substr(0, restLen);
			}
			if (s.length === before || !s.length) break;
		}
		return s;
	};

	this.groupName = function(name)
	{
		return this.normalize(this.stripCopySuffix(name));
	};

	this.groupKey = function(name)
	{
		var g = this.groupName(name);
		return g ? GROUP_KEY_PREFIX + g : '';
	};

	/*
	 * Plans how the Add dialog sends its files.
	 *   files: [{ name, content, dupekey }], content compared for exact copies
	 *     (may be undefined when not read yet), dupekey typed by the user.
	 *   opts: { enabled (the "one fleet" box), force (DupeForce) }.
	 * Returns { fleets: [{ key, title, members: [index] }], singles: [index],
	 *   dropped: [{ index, sameAs }] }. With the box off or Force on every file
	 *   is a single, as before. Otherwise exact copies of an earlier file are
	 *   dropped, and files whose group names match form a fleet of two or more
	 *   under the DupeKey a member's user typed, else dupes:<group name>.
	 */
	this.plan = function(files, opts)
	{
		var result = { fleets: [], singles: [], dropped: [] };
		var i;
		if (!opts || !opts.enabled || opts.force)
		{
			for (i = 0; i < files.length; i++) result.singles.push(i);
			return result;
		}
		var kept = [];
		for (i = 0; i < files.length; i++)
		{
			var same = -1;
			if (files[i].content !== undefined && files[i].content !== null)
			{
				for (var k = 0; k < kept.length; k++)
				{
					if (files[kept[k]].content === files[i].content)
					{
						same = kept[k];
						break;
					}
				}
			}
			if (same > -1)
			{
				result.dropped.push({ index: i, sameAs: same });
			}
			else
			{
				kept.push(i);
			}
		}
		var groups = [], byName = {};
		for (i = 0; i < kept.length; i++)
		{
			var idx = kept[i];
			var g = this.groupName(files[idx].name);
			if (!g)
			{
				groups.push({ name: '', members: [idx] });
				continue;
			}
			if (!byName.hasOwnProperty(g))
			{
				byName[g] = groups.length;
				groups.push({ name: g, members: [] });
			}
			groups[byName[g]].members.push(idx);
		}
		for (i = 0; i < groups.length; i++)
		{
			var grp = groups[i];
			if (grp.members.length < 2)
			{
				result.singles.push(grp.members[0]);
				continue;
			}
			var key = '';
			for (var m = 0; m < grp.members.length && !key; m++)
			{
				key = files[grp.members[m]].dupekey || '';
			}
			result.fleets.push({
				key: key || GROUP_KEY_PREFIX + grp.name,
				title: this.stripCopySuffix(files[grp.members[0]].name),
				members: grp.members
			});
		}
		result.singles.sort(function(a, b) { return a - b; });
		return result;
	};
}());

if (typeof module !== 'undefined' && module.exports)
{
	module.exports = DupeGroup;
}
