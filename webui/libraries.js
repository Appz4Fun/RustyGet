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
 * The one list of every library RustyGet ships, used by the License page and
 * the libraries table on the Status page.
 *
 * Engine entries match the crate names the daemon reports in `sysinfo`
 * (BUNDLED_LIBRARIES in rust/src/systemhealth.rs) plus the OpenSSL runtime.
 * `internal` marks libraries developed and copyrighted by xbmc4lyfe; the goal
 * is for every entry to become internal, and the License page shows how far
 * along that is.
 */
var Libraries = (new function()
{
	'use strict';

	var OWNER = 'xbmc4lyfe';

	var CATALOG = [
		// --- engine (Rust crates) ---
		{ name: 'zipfast', kind: 'engine', internal: true, license: 'RPL-1.5', author: OWNER,
			url: 'https://github.com/xbmc4lyfe/zipfast',
			desc: 'Archive inspection and extraction: RAR, 7-Zip, ZIP, TAR and compressed streams, nested archives and live hole skipping.' },
		{ name: 'zipview', kind: 'engine', internal: true, license: 'GPL-3.0', author: OWNER,
			url: 'https://github.com/xbmc4lyfe/zipview',
			desc: 'Read-only, allocation-bounded ZIP container reader used to look inside releases without extracting them.' },
		{ name: 'par-rs', kind: 'engine', internal: true, license: 'GPL-2.0-or-later', author: OWNER,
			url: 'https://github.com/xbmc4lyfe/par-rs',
			desc: 'SIMD PAR2 verification and Reed-Solomon repair, including repair while downloading.' },
		{ name: 'nntp-rs', kind: 'engine', license: 'AGPL-3.0-or-later AND GPL-2.0-or-later',
			author: OWNER + ', thedancingdeveloper and the NZBGet contributors',
			url: 'https://github.com/Appz4Fun/nntp-rs',
			desc: 'News server connections (one pool for downloads, repair, checks and probes), article decoding (SIMD yEnc, CRC checks) and finding the servers that share an article store.' },
		{ name: 'flate2', kind: 'engine', license: 'MIT / Apache-2.0', author: 'The Rust Project Developers',
			url: 'https://github.com/rust-lang/flate2-rs',
			desc: 'Deflate, zlib and gzip streams.' },
		{ name: 'zlib-rs', kind: 'engine', license: 'Zlib', author: 'Trifecta Tech Foundation',
			url: 'https://github.com/trifectatechfoundation/zlib-rs',
			desc: 'Pure-Rust zlib backend behind flate2.' },
		{ name: 'zstd', kind: 'engine', license: 'MIT / Apache-2.0', author: 'Alexandre Bury; libzstd by Meta Platforms (BSD-3-Clause)',
			url: 'https://github.com/gyscos/zstd-rs',
			desc: 'Zstandard decompression.' },
		{ name: 'bzip2', kind: 'engine', license: 'MIT / Apache-2.0', author: 'Trifecta Tech Foundation and contributors',
			url: 'https://github.com/trifectatechfoundation/bzip2-rs',
			desc: 'Bzip2 decompression on the pure-Rust libbz2-rs backend.' },
		{ name: 'crc32fast', kind: 'engine', license: 'MIT / Apache-2.0', author: 'Sam Rijs, Alex Crichton',
			url: 'https://github.com/srijs/rust-crc32fast',
			desc: 'SIMD CRC32 for article and file checks.' },
		{ name: 'quick-xml', kind: 'engine', license: 'MIT', author: 'Johann Tuffe and contributors',
			url: 'https://github.com/tafia/quick-xml',
			desc: 'NZB and RSS parsing.' },
		{ name: 'md-5', kind: 'engine', license: 'MIT / Apache-2.0', author: 'RustCrypto Developers',
			url: 'https://github.com/RustCrypto/hashes',
			desc: 'MD5 hashes for PAR2 file matching.' },
		{ name: 'libc', kind: 'engine', license: 'MIT / Apache-2.0', author: 'The Rust Project Developers',
			url: 'https://github.com/rust-lang/libc',
			desc: 'Raw bindings to the operating system.' },
		{ name: 'openssl', kind: 'engine', license: 'Apache-2.0', author: 'Steven Fackler',
			url: 'https://github.com/sfackler/rust-openssl',
			desc: 'Rust bindings to OpenSSL.' },
		{ name: 'OpenSSL', kind: 'engine', license: 'Apache-2.0', author: 'The OpenSSL Project',
			url: 'https://github.com/openssl/openssl',
			desc: 'TLS for news servers and the web interface (system library).' },

		// --- web interface ---
		{ name: 'jQuery', kind: 'webui', version: '1.7.2', license: 'MIT', author: 'OpenJS Foundation and contributors',
			url: 'https://github.com/jquery/jquery',
			desc: 'DOM and AJAX helpers.' },
		{ name: 'Bootstrap', kind: 'webui', version: '2.0.3', license: 'Apache-2.0', author: 'Twitter, Inc.',
			url: 'https://github.com/twbs/bootstrap',
			desc: 'Base styles, dropdowns and dialogs.' },
		{ name: 'Raphaël', kind: 'webui', version: '2.1.0', license: 'MIT', author: 'Dmitry Baranovskiy',
			url: 'https://github.com/DmitryBaranovskiy/raphael',
			desc: 'Vector drawing for the per-server charts.' },
		{ name: 'Elycharts', kind: 'webui', version: '2.1.6', license: 'MIT', author: 'Void Labs s.n.c.',
			url: 'https://github.com/voidlabs/elycharts',
			desc: 'Per-server speed and volume charts.' },
		{ name: 'Material Symbols Rounded', kind: 'webui', license: 'Apache-2.0', author: 'Google',
			url: 'https://github.com/google/material-design-icons',
			desc: 'Icon font.' },
		{ name: 'Azeret Mono', kind: 'webui', license: 'OFL-1.1', author: 'The Azeret Project Authors',
			url: 'https://github.com/displaay/azeret',
			desc: 'Monospace typeface for numbers and labels.' }
	];

	this.OWNER = OWNER;

	this.all = function()
	{
		return CATALOG.slice();
	};

	// Exact name first ("OpenSSL" the runtime vs "openssl" the crate), then any case.
	this.find = function(name)
	{
		var text = String(name || '');
		var lower = text.toLowerCase();
		var loose = null;
		for (var i = 0; i < CATALOG.length; i++)
		{
			if (CATALOG[i].name === text)
			{
				return CATALOG[i];
			}
			if (!loose && CATALOG[i].name.toLowerCase() === lower)
			{
				loose = CATALOG[i];
			}
		}
		return loose;
	};

	// { internal, total, percent }
	this.stats = function()
	{
		var internal = CATALOG.filter(function(l) { return l.internal; }).length;
		return {
			internal: internal,
			total: CATALOG.length,
			percent: CATALOG.length ? Math.round(internal * 100 / CATALOG.length) : 0
		};
	};

	// Daemon-reported libraries (Name/Version) merged with the catalog, then
	// the web interface libraries. Unknown daemon entries are kept as-is.
	this.merge = function(reported)
	{
		var seen = {};
		var rows = [];
		(reported || []).forEach(function(lib)
		{
			var entry = Libraries.find(lib.Name) || { name: lib.Name, kind: 'engine' };
			seen[entry.name] = true;
			rows.push($.extend({}, entry, { version: lib.Version || entry.version || '' }));
		});
		CATALOG.forEach(function(entry)
		{
			if (!seen[entry.name] && entry.kind === 'webui')
			{
				rows.push($.extend({}, entry));
			}
		});
		return rows;
	};

	function link(entry, text)
	{
		var $a = $('<a target="_blank" rel="noopener noreferrer">').attr('href', entry.url).text(text);
		return $a;
	}

	this.link = link;

	function licenseCard(entry)
	{
		var $card = $('<article class="license-card">').toggleClass('license-card--internal', !!entry.internal);
		var $head = $('<div class="license-card__head">');
		$head.append($('<h5 class="license-card__name">').append(entry.url ? link(entry, entry.name) : document.createTextNode(entry.name)));
		if (entry.internal)
		{
			$head.append($('<span class="license-badge license-badge--internal">').text(OWNER));
		}
		$head.append($('<span class="license-badge">').text(entry.license));
		$card.append($head);
		$card.append($('<p class="license-card__desc">').text(entry.desc || ''));
		$card.append($('<p class="license-card__meta">').text((entry.internal ? 'Copyright © ' : '© ') + entry.author));
		return $card;
	}

	// Fills the License page.
	this.renderLicensePage = function($root)
	{
		if (!$root || !$root.length) return;
		var s = this.stats();
		var $list = $root.find('#LicenseLibraries').empty();

		$root.find('#LicenseInternalPercent').text(s.percent + '%');
		$root.find('#LicenseInternalCount').text(s.internal + ' of ' + s.total + ' libraries are internal');
		$root.find('#LicenseInternalBar').css('width', s.percent + '%')
			.attr('aria-valuenow', s.percent);

		[
			{ kind: 'engine', internal: true, title: 'Internal libraries by ' + OWNER },
			{ kind: 'engine', internal: false, title: 'Engine libraries' },
			{ kind: 'webui', internal: false, title: 'Web interface' }
		].forEach(function(group)
		{
			var entries = CATALOG.filter(function(l) { return l.kind === group.kind && !!l.internal === group.internal; });
			if (!entries.length) return;
			var $section = $('<section class="license-group">');
			$section.append($('<h4 class="license-group__title">').text(group.title + ' (' + entries.length + ')'));
			var $grid = $('<div class="license-grid">');
			entries.forEach(function(entry) { $grid.append(licenseCard(entry)); });
			$section.append($grid);
			$list.append($section);
		});
	};
}());
