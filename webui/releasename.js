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
 * Release names as people read them. "The.Wheel.of.Time.S02E02.2160p.HDR10Plus
 * .DV.WEBRip.x265-PSA" becomes the title "The Wheel of Time", episode S02E02 and
 * tags 2160p, WEBRip, HDR10+, DV, x265 and the group PSA.
 *
 * The Downloads list shows the clean title with its tags and the original name
 * underneath, unchanged. When the NZB name is obfuscated (a hash or random
 * string) the title comes from the best other name we have: a name the API
 * passed as a parameter, or the NZB file name.
 */
var ReleaseName = (new function()
{
	'use strict';

	var RESOLUTION = /^(4320p|2160p|1440p|1080p|1080i|720p|576p|576i|540p|480p|480i|360p|8k|4k|uhd)$/i;
	var SOURCE = {
		'web-dl': 'WEB-DL', 'webdl': 'WEB-DL', 'webrip': 'WEBRip', 'web': 'WEB', 'bluray': 'BluRay', 'blu-ray': 'BluRay',
		'bdrip': 'BDRip', 'brrip': 'BRRip', 'bdremux': 'Remux', 'remux': 'Remux', 'hdtv': 'HDTV', 'pdtv': 'PDTV',
		'dvdrip': 'DVDRip', 'dvd': 'DVD', 'dvdr': 'DVD-R', 'hdrip': 'HDRip', 'uhdbd': 'UHD BluRay', 'hddvd': 'HD DVD',
		'amzn': 'AMZN', 'nf': 'NF', 'dsnp': 'DSNP', 'hmax': 'HMAX', 'max': 'MAX', 'atvp': 'ATVP', 'hulu': 'HULU', 'pcok': 'PCOK',
		'pmtp': 'PMTP', 'stan': 'STAN', 'it': null
	};
	var CODEC = {
		'x264': 'x264', 'x265': 'x265', 'h264': 'H.264', 'h.264': 'H.264', 'h265': 'H.265', 'h.265': 'H.265',
		'hevc': 'HEVC', 'avc': 'AVC', 'av1': 'AV1', 'xvid': 'XviD', 'divx': 'DivX', 'vp9': 'VP9', 'mpeg2': 'MPEG-2'
	};
	var HDR = {
		'hdr': 'HDR', 'hdr10': 'HDR10', 'hdr10plus': 'HDR10+', 'hdr10+': 'HDR10+', 'dv': 'DV', 'dovi': 'DV',
		'dolbyvision': 'DV', 'hlg': 'HLG', 'sdr': 'SDR'
	};
	var AUDIO = {
		'aac': 'AAC', 'ac3': 'AC3', 'dd': 'DD', 'dd5.1': 'DD 5.1', 'ddp': 'DD+', 'ddp5.1': 'DD+ 5.1', 'eac3': 'EAC3',
		'dts': 'DTS', 'dts-hd': 'DTS-HD', 'dtshd': 'DTS-HD', 'dts-x': 'DTS:X', 'truehd': 'TrueHD', 'atmos': 'Atmos',
		'flac': 'FLAC', 'opus': 'Opus', 'mp3': 'MP3', '6ch': '5.1', '8ch': '7.1', '5.1': '5.1', '7.1': '7.1', '2.0': '2.0'
	};
	var EDITION = {
		'proper': 'PROPER', 'repack': 'REPACK', 'rerip': 'RERIP', 'extended': 'Extended', 'unrated': 'Unrated',
		'directors': "Director's Cut", 'remastered': 'Remastered', 'imax': 'IMAX', 'internal': 'iNTERNAL', 'limited': 'LIMITED',
		'complete': 'Complete', 'multi': 'MULTi', 'dubbed': 'Dubbed', 'subbed': 'Subbed', 'hybrid': 'Hybrid', 'uncut': 'Uncut'
	};
	var NOISE = /^(nzbdav|nzbgeek|nzb|sample|rarbg|eztv|ettv|tgx|www)$/i;

	function stripName(name)
	{
		var s = String(name || '').trim();
		// a copy suffix (-FraMeSToR_3434, " (12)", _1.nzb.2.queued) is no part of
		// the release (the strict rule of webui/dupegroup.js and releasename.rs)
		if (typeof DupeGroup !== 'undefined') s = DupeGroup.stripCopySuffix(s);
		s = s.replace(/\.(nzb|mkv|mp4|avi|m4v|ts|par2|rar|7z|zip)$/i, '');
		// trailing bracket tags such as "[NZBDAV copy 01]" or "{tvdb-123}"
		s = s.replace(/(\s*[\[{(][^\]})]*[\]})]\s*)+$/, '');
		return s.trim();
	}

	// True for names that carry no words: hashes, random strings, bare numbers.
	function isObfuscated(name)
	{
		var s = stripName(name);
		if (!s) return true;
		if (/^[a-f0-9]{16,}$/i.test(s)) return true;
		if (/^[A-Za-z0-9]{20,}$/.test(s) && !/[._ -]/.test(s)) return true;
		if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s)) return true;
		if (/^\d+$/.test(s)) return true;
		// mostly consonant soup without separators
		if (s.length >= 12 && !/[._ -]/.test(s) && !/[aeiou]{1}[a-z]*[aeiou]/i.test(s.replace(/[^a-z]/gi, ''))) return true;
		return false;
	}

	function titleCase(words)
	{
		var text = words.join(' ').replace(/\s+/g, ' ').trim();
		// an all-lowercase title ("show name") reads better capitalized
		if (text && text === text.toLowerCase())
		{
			text = text.replace(/(^|\s)([a-z])/g, function(m, sp, c) { return sp + c.toUpperCase(); });
		}
		return text;
	}

	function parse(name)
	{
		var original = String(name || '');
		var s = stripName(original);
		var r = { original: original, title: '', year: '', episode: '', date: '', resolution: '', source: '', codec: '',
			hdr: [], audio: [], edition: [], group: '', tags: [], parsed: false };
		if (!s) return r;

		// release group: the text after the last dash, when it is one word
		var m = s.match(/-([A-Za-z0-9]+(?:\.[A-Za-z0-9]+)?)$/);
		var body = s;
		// "S01E01-E02" ends in an episode range, not a group
		var range = m && /^E\d{1,4}$/i.test(m[1]) && /S\d{1,3}E\d{1,4}$/i.test(s.slice(0, m.index));
		if (m && !range && !RESOLUTION.test(m[1]) && !/^\d{1,2}$/.test(m[1]) && !/^(dl|rip|hd|x|ray)$/i.test(m[1]))
		{
			r.group = m[1];
			body = s.slice(0, m.index);
		}

		// keep audio channel counts such as DDP5.1 or 7.1 in one token
		body = body.replace(/(^|[._ -])((?:ddp?|dd\+|aac|dts|ac3|eac3|truehd|flac|opus)?[2-9])\.([01])(?=[._ -]|$)/gi, '$1$2~$3');
		var tokens = body.replace(/[._]+/g, ' ').replace(/\s+/g, ' ').trim().split(' ').map(function(w) { return w.replace(/~/g, '.'); });
		var titleEnd = -1;
		var markTitle = function(i) { if (titleEnd < 0 || i < titleEnd) titleEnd = i; };
		// Streaming services and edition words are also ordinary title words ("Mad Max",
		// "Uncut Gems"), so they only count once we know where the title ends.
		var ambiguous = [];

		for (var i = 0; i < tokens.length; i++)
		{
			var t = tokens[i];
			var lower = t.toLowerCase();
			var next = (tokens[i + 1] || '').toLowerCase();
			var ep;

			if ((ep = t.match(/^S(\d{1,3})(?:[ ._-]?E(\d{1,4}))?(?:[-]?E(\d{1,4}))?$/i)))
			{
				r.episode = 'S' + pad(ep[1]) + (ep[2] ? 'E' + pad(ep[2]) : '') + (ep[3] ? '-E' + pad(ep[3]) : '');
				// "S01.E02" and "S01 E02" split the episode into its own token
				var sep = !ep[2] && (tokens[i + 1] || '').match(/^E(\d{1,4})$/i);
				if (sep)
				{
					r.episode += 'E' + pad(sep[1]);
					i++;
				}
				markTitle(i - (sep ? 1 : 0));
			}
			else if ((ep = t.match(/^(\d{1,2})x(\d{2,3})$/i)))
			{
				r.episode = 'S' + pad(ep[1]) + 'E' + pad(ep[2]);
				markTitle(i);
			}
			else if (/^(19|20)\d{2}$/.test(t) && /^\d{2}$/.test(tokens[i + 1] || '') && /^\d{2}$/.test(tokens[i + 2] || ''))
			{
				r.date = t + '-' + tokens[i + 1] + '-' + tokens[i + 2];
				markTitle(i);
				i += 2;
			}
			else if (/^\(?(19|20)\d{2}\)?$/.test(t) && i > 0)
			{
				// the last year before any other marker ends the title, so a year that is
				// part of the title ("Blade Runner 2049 2017") stays in it
				if (titleEnd < 0)
				{
					r.year = t.replace(/[()]/g, '');
					r._yearAt = i;
				}
			}
			else if (RESOLUTION.test(t))
			{
				r.resolution = lower === '4k' || lower === 'uhd' ? '2160p' : lower === '8k' ? '4320p' : lower;
				markTitle(i);
			}
			else if (lower === 'web' && next === 'dl')
			{
				r.source = 'WEB-DL';
				markTitle(i);
				i++;
			}
			else if (Object.prototype.hasOwnProperty.call(SOURCE, lower) && SOURCE[lower])
			{
				if (/^(amzn|nf|dsnp|hmax|max|atvp|hulu|pcok|pmtp|stan)$/.test(lower))
				{
					ambiguous.push({ at: i, edition: SOURCE[lower] });
				}
				else
				{
					r.source = SOURCE[lower];
					markTitle(i);
				}
			}
			else if (Object.prototype.hasOwnProperty.call(CODEC, lower))
			{
				if (!r.codec) r.codec = CODEC[lower];
				markTitle(i);
			}
			else if ((lower === 'h' || lower === 'x') && (next === '264' || next === '265'))
			{
				r.codec = lower === 'x' ? 'x' + next : 'H.' + next;
				markTitle(i);
				i++;
			}
			else if (Object.prototype.hasOwnProperty.call(HDR, lower))
			{
				if (r.hdr.indexOf(HDR[lower]) < 0) r.hdr.push(HDR[lower]);
				markTitle(i);
			}
			else if (Object.prototype.hasOwnProperty.call(AUDIO, lower) || /^(ddp?|eac3|dts|aac|ac3|truehd|flac|opus)\d(\.\d)?$/.test(lower))
			{
				var am = lower.match(/^([a-z]+)(\d\.\d)$/);
				var a = AUDIO[lower] || (am ? (AUDIO[am[1]] || am[1].toUpperCase()) + ' ' + am[2] : t.toUpperCase());
				if (r.audio.indexOf(a) < 0) r.audio.push(a);
				markTitle(i);
			}
			else if (Object.prototype.hasOwnProperty.call(EDITION, lower))
			{
				ambiguous.push({ at: i, edition: EDITION[lower] });
			}
		}

		// the year ends the title when it comes before the first marker
		if (r.year && r._yearAt !== undefined && (titleEnd < 0 || r._yearAt < titleEnd))
		{
			titleEnd = r._yearAt;
		}
		delete r._yearAt;

		// An ambiguous word ends the title only when nothing but metadata follows it
		// up to the first certain marker (or the end of the name).
		var isAmbiguous = {};
		ambiguous.forEach(function(a) { isAmbiguous[a.at] = true; });
		var end = titleEnd < 0 ? tokens.length : titleEnd;
		while (end > 0 && isAmbiguous[end - 1]) end--;
		if (end < (titleEnd < 0 ? tokens.length : titleEnd)) titleEnd = end;
		ambiguous.forEach(function(a)
		{
			if (titleEnd >= 0 && a.at >= titleEnd && r.edition.indexOf(a.edition) < 0) r.edition.push(a.edition);
		});

		var titleTokens = (titleEnd < 0 ? tokens : tokens.slice(0, titleEnd)).filter(function(w) { return w && !NOISE.test(w); });
		r.title = titleCase(titleTokens);
		r.parsed = titleEnd > 0 && !!r.title;
		if (!r.title) r.title = s.replace(/[._]+/g, ' ');

		// the row shows what tells copies apart; audio and streaming service stay in r.audio / r.edition
		var notable = r.edition.filter(function(e) { return /^(PROPER|REPACK|RERIP|Remastered|Extended|IMAX|Hybrid)$/.test(e); });
		r.tags = [].concat(r.resolution ? [r.resolution] : [], r.source ? [r.source] : [], r.hdr, r.codec ? [r.codec] : [],
			notable, r.group ? [r.group] : []);
		return r;
	}

	function pad(n)
	{
		n = String(+n);
		return n.length < 2 ? '0' + n : n;
	}

	function param(group, names)
	{
		var params = group.Parameters || [];
		for (var i = 0; i < names.length; i++)
		{
			for (var j = 0; j < params.length; j++)
			{
				if (params[j].Name.toLowerCase() === names[i].toLowerCase() && params[j].Value)
				{
					return params[j].Value;
				}
			}
		}
		return '';
	}

	// The name to parse for a queue group: the NZB name, or when that is
	// obfuscated a release name the API passed, or the NZB file name.
	function bestName(group)
	{
		var nzbName = group.NZBName || '';
		if (!isObfuscated(nzbName)) return nzbName;
		var candidates = [
			param(group, ['ReleaseName', 'OriginalName', 'ProperName', 'Title', '*ReleaseName']),
			(group.NZBFilename || '').replace(/^.*[\\\/]/, ''),
			group.NZBNicename || ''
		];
		// a readable name that parses as a release wins over one that is only readable
		var readable = candidates.filter(function(c) { return c && !isObfuscated(c); });
		for (var i = 0; i < readable.length; i++)
		{
			if (parse(readable[i]).parsed) return readable[i];
		}
		return readable.length ? readable[0] : nzbName;
	}

	// { title, episode, year, tags, original, source } for the Downloads list.
	function forGroup(group)
	{
		var name = bestName(group);
		var r = parse(name);
		var headline = r.title + (r.episode ? ' ' + r.episode : r.date ? ' ' + r.date : '') + (r.year && !r.episode ? ' (' + r.year + ')' : '');
		return { headline: headline, parsed: r.parsed, tags: r.tags, original: group.NZBName || '', from: name, release: r };
	}

	this.parse = parse;
	this.isObfuscated = isObfuscated;
	this.bestName = bestName;
	this.forGroup = forGroup;

	// HTML for the name cell of a queue row. `link` is the anchor open tag.
	this.nameHtml = function(group, link)
	{
		var esc = Util.textToHtml;
		var d = forGroup(group);
		if (!d.parsed)
		{
			if (d.from === d.original)
			{
				return link + esc(Util.formatNZBName(group.NZBName)) + '</a>';
			}
			// a readable name from the API or file name, with the obfuscated NZB name under it
			return link + esc(Util.formatNZBName(d.from)) + '</a>' +
				'<div class="release-original">NZB ' + esc(d.original) + '</div>';
		}
		var html = link + esc(d.headline) + '</a>';
		d.tags.forEach(function(tag) { html += ' <span class="release-tag">' + esc(tag) + '</span>'; });
		// the real name, unchanged; an obfuscated NZB name is kept beside it
		var under = d.from + (d.from !== d.original ? ' \u00b7 NZB ' + d.original : '');
		html += '<div class="release-original">' + esc(under) + '</div>';
		return html;
	};
}());
