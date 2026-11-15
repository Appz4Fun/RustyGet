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
 * Machine metrics on the Status page (tiles) and the Statistics page (tiles
 * plus charts of speed, connections, load, CPU and RAM over time).
 *
 * Data comes from the `systemstats` RPC: the current reading and a history the
 * daemon samples every 5 seconds (one hour) and averages per minute (24 hours),
 * so the charts cover time the page was closed. Charts are plain inline SVG.
 */
var SystemMetrics = (new function($)
{
	'use strict';

	var POLL_MS = 5000;
	var RANGES = [
		{ sec: 900, label: '15m' },
		{ sec: 3600, label: '1h' },
		{ sec: 6 * 3600, label: '6h' },
		{ sec: 24 * 3600, label: '24h' }
	];
	var SVG_NS = 'http://www.w3.org/2000/svg';

	var range = 3600;
	var timer = null;
	var loading = false;
	var last = null;
	var hovering = false;

	// ---- formatting ----

	function bytes(n)
	{
		n = +n || 0;
		var units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
		var i = 0;
		while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
		return (i === 0 ? n.toFixed(0) : n.toFixed(n >= 100 ? 0 : 1)) + ' ' + units[i];
	}

	function speed(n)
	{
		n = +n || 0;
		var units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
		var i = 0;
		while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
		return (i === 0 ? n.toFixed(0) : n.toFixed(n >= 100 ? 0 : 1)) + ' ' + units[i];
	}

	function pct(n)
	{
		n = +n || 0;
		return (n >= 10 ? n.toFixed(0) : n.toFixed(1)) + '%';
	}

	function load(n)
	{
		return (+n || 0).toFixed(2);
	}

	function clock(t, withSeconds)
	{
		var d = new Date(t * 1000);
		var p = function(v) { return v < 10 ? '0' + v : '' + v; };
		return p(d.getHours()) + ':' + p(d.getMinutes()) + (withSeconds ? ':' + p(d.getSeconds()) : '');
	}

	this.format = { bytes: bytes, speed: speed, pct: pct, load: load };

	// ---- tiles ----

	function tile(label, value, sub, icon)
	{
		var $t = $('<div class="details-tile sysmetrics-tile">');
		var $label = $('<span class="details-tile-label">');
		if (icon) $label.append($('<i class="material-icon">').text(icon));
		$label.append(document.createTextNode(label));
		$t.append($label);
		$t.append($('<span class="sysmetrics-tile__value">').text(value));
		if (sub) $t.append($('<span class="sysmetrics-tile__sub">').text(sub));
		return $t;
	}

	this.tiles = function(d)
	{
		var mem = d.Memory || {};
		var swap = d.Swap || {};
		var cpu = d.Cpu || {};
		var swapUsed = Math.max(0, (swap.Total || 0) - (swap.Free || 0));
		var cpuName = [cpu.Vendor && cpu.Model && cpu.Model.indexOf(cpu.Vendor) === -1 ? cpu.Vendor : '', cpu.Model].join(' ').trim();
		var memPct = mem.Total ? mem.Used * 100 / mem.Total : 0;
		var load = d.Load || [0, 0, 0];

		return [
			tile('Load average', load.map(SystemMetrics.format.load).join('  '), '1, 5 and 15 minutes · ' + d.CpuCount + ' CPUs', 'speed'),
			tile('CPU', pct(d.CpuPercent) + ' machine', pct(d.ProcessCpuPercent) + ' RustyGet', 'percent'),
			tile('Processor', (d.CpuCount || 0) + ' × ' + (cpuName || 'unknown CPU'),
				[cpu.Generation, cpu.Arch].filter(Boolean).join(' · '), 'computer'),
			tile('RAM', bytes(mem.Used) + ' of ' + bytes(mem.Total), pct(memPct) + ' used · ' + bytes(mem.Available) + ' available', 'stack'),
			tile('RAM free', bytes(mem.Free), bytes(mem.Process) + ' used by RustyGet', 'package'),
			tile('Swap', swap.Total ? bytes(swapUsed) + ' of ' + bytes(swap.Total) : 'none', swap.Total ? bytes(swap.Free) + ' free' : '', 'sync'),
			tile('Memory compression', d.MemoryCompression || 'none',
				'zswap ' + (d.Zswap ? 'on' : 'off') + ' · zram ' + (d.Zram ? 'on' : 'off'), 'scale')
		];
	};

	function renderTiles($el, d)
	{
		if (!$el || !$el.length) return;
		$el.empty();
		SystemMetrics.tiles(d).forEach(function($t) { $el.append($t); });
	}

	// ---- charts ----

	function svg(tag, attrs)
	{
		var el = document.createElementNS(SVG_NS, tag);
		for (var k in attrs)
		{
			if (Object.prototype.hasOwnProperty.call(attrs, k)) el.setAttribute(k, attrs[k]);
		}
		return el;
	}

	function niceMax(v)
	{
		if (!(v > 0)) return 1;
		var exp = Math.pow(10, Math.floor(Math.log(v) / Math.LN10));
		var f = v / exp;
		var nice = f <= 1 ? 1 : f <= 2 ? 2 : f <= 2.5 ? 2.5 : f <= 5 ? 5 : 10;
		return nice * exp;
	}

	function niceBytesMax(v, base)
	{
		// round up on a 1024 scale so axis labels read as whole units
		if (!(v > 0)) return base;
		var unit = 1;
		while (v / unit >= 1024) unit *= 1024;
		return niceMax(v / unit) * unit;
	}

	// spec: { id, title, series: [{ name, cls, values }], times, interval, fmt, max }
	this.chart = function(spec)
	{
		var $wrap = $('<figure class="sysmetrics-chart">').attr('id', spec.id);
		var $head = $('<figcaption class="sysmetrics-chart__head">');
		$head.append($('<span class="sysmetrics-chart__title">').text(spec.title));
		var $now = $('<span class="sysmetrics-chart__now">');
		$head.append($now);
		$wrap.append($head);

		if (spec.series.length > 1)
		{
			var $legend = $('<div class="sysmetrics-legend">');
			spec.series.forEach(function(s)
			{
				$legend.append($('<span class="sysmetrics-legend__item">')
					.append($('<i class="sysmetrics-swatch">').addClass(s.cls))
					.append(document.createTextNode(s.name)));
			});
			$wrap.append($legend);
		}

		var latest = spec.series.map(function(s) {
			var v = s.values.length ? s.values[s.values.length - 1] : null;
			return v === null ? '' : (spec.series.length > 1 ? s.name + ' ' : '') + spec.fmt(v);
		}).filter(Boolean).join(' · ');
		$now.text(latest);

		var $plot = $('<div class="sysmetrics-chart__plot">');
		$wrap.append($plot);
		$wrap.data('spec', spec);
		return $wrap;
	};

	function drawPlot($wrap)
	{
		var spec = $wrap.data('spec');
		var $plot = $wrap.find('.sysmetrics-chart__plot');
		$plot.empty();
		var width = Math.max(240, Math.floor($plot.width() || 600));
		var height = 150;
		var pad = { l: 56, r: 8, t: 8, b: 22 };
		var w = width - pad.l - pad.r;
		var h = height - pad.t - pad.b;
		var times = spec.times;
		var n = times.length;

		var root = svg('svg', { width: width, height: height, viewBox: '0 0 ' + width + ' ' + height,
			role: 'img', 'aria-label': spec.title, 'class': 'sysmetrics-svg' });

		var dataMax = 0;
		spec.series.forEach(function(s) { s.values.forEach(function(v) { if (v > dataMax) dataMax = v; }); });
		var max = spec.max || (spec.bytes ? niceBytesMax(dataMax, 1024) : niceMax(dataMax));
		var t1 = n ? times[n - 1] : Math.floor(Date.now() / 1000);
		var t0 = t1 - spec.range;
		var x = function(t) { return pad.l + (t - t0) / (t1 - t0) * w; };
		var y = function(v) { return pad.t + h - Math.min(v, max) / max * h; };

		// grid and y labels
		for (var g = 0; g <= 4; g++)
		{
			var gv = max * g / 4;
			var gy = y(gv);
			root.appendChild(svg('line', { x1: pad.l, x2: pad.l + w, y1: gy, y2: gy, 'class': g === 0 ? 'sysmetrics-axis' : 'sysmetrics-grid' }));
			var label = svg('text', { x: pad.l - 6, y: gy + 3, 'text-anchor': 'end', 'class': 'sysmetrics-tick' });
			label.textContent = spec.fmt(gv);
			root.appendChild(label);
		}
		// x labels
		for (var k = 0; k <= 4; k++)
		{
			var tt = t0 + (t1 - t0) * k / 4;
			var xl = svg('text', { x: x(tt), y: height - 6, 'text-anchor': k === 0 ? 'start' : k === 4 ? 'end' : 'middle', 'class': 'sysmetrics-tick' });
			xl.textContent = clock(tt);
			root.appendChild(xl);
		}

		if (!n)
		{
			var empty = svg('text', { x: pad.l + w / 2, y: pad.t + h / 2, 'text-anchor': 'middle', 'class': 'sysmetrics-tick' });
			empty.textContent = 'Collecting samples…';
			root.appendChild(empty);
			$plot.append(root);
			return;
		}

		// lines, broken where samples are missing (daemon was stopped)
		var gap = spec.interval * 3;
		spec.series.forEach(function(s)
		{
			var d = '';
			for (var i = 0; i < n; i++)
			{
				if (times[i] < t0) continue;
				var move = d === '' || times[i] - times[i - 1] > gap;
				d += (move ? 'M' : 'L') + x(times[i]).toFixed(1) + ',' + y(s.values[i]).toFixed(1);
			}
			if (d) root.appendChild(svg('path', { d: d, 'class': 'sysmetrics-line ' + s.cls }));
		});

		// hover: crosshair, dots and a tooltip
		var cross = svg('line', { y1: pad.t, y2: pad.t + h, 'class': 'sysmetrics-cross', visibility: 'hidden' });
		root.appendChild(cross);
		var dots = spec.series.map(function(s)
		{
			var c = svg('circle', { r: 4, 'class': 'sysmetrics-dot ' + s.cls, visibility: 'hidden' });
			root.appendChild(c);
			return c;
		});
		var hit = svg('rect', { x: pad.l, y: 0, width: w, height: height, fill: 'transparent', 'class': 'sysmetrics-hit' });
		root.appendChild(hit);
		$plot.append(root);
		var $tip = $('<div class="sysmetrics-tip" role="status">').hide();
		$plot.append($tip);

		function show(clientX)
		{
			var rect = root.getBoundingClientRect();
			var t = t0 + (clientX - rect.left - pad.l) / w * (t1 - t0);
			var best = -1, bestD = Infinity;
			for (var i = 0; i < n; i++)
			{
				var dd = Math.abs(times[i] - t);
				if (dd < bestD) { bestD = dd; best = i; }
			}
			if (best < 0 || times[best] < t0) return hide();
			var cx = x(times[best]);
			cross.setAttribute('x1', cx);
			cross.setAttribute('x2', cx);
			cross.setAttribute('visibility', 'visible');
			$tip.empty().append($('<div class="sysmetrics-tip__time">').text(clock(times[best], spec.interval < 60)));
			spec.series.forEach(function(s, si)
			{
				dots[si].setAttribute('cx', cx);
				dots[si].setAttribute('cy', y(s.values[best]));
				dots[si].setAttribute('visibility', 'visible');
				$tip.append($('<div class="sysmetrics-tip__row">')
					.append($('<i class="sysmetrics-swatch">').addClass(s.cls))
					.append($('<span>').text(s.name))
					.append($('<strong>').text(spec.fmt(s.values[best]))));
			});
			var left = Math.min(Math.max(cx + 12, 0), width - 170);
			if (cx > width - 190) left = cx - 170;
			$tip.css({ left: Math.max(0, left) + 'px', top: pad.t + 'px' }).show();
		}
		function hide()
		{
			cross.setAttribute('visibility', 'hidden');
			dots.forEach(function(c) { c.setAttribute('visibility', 'hidden'); });
			$tip.hide();
		}
		hit.addEventListener('mousemove', function(e) { hovering = true; show(e.clientX); });
		hit.addEventListener('mouseleave', function() { hovering = false; hide(); });
		hit.addEventListener('touchstart', function(e) { if (e.touches[0]) show(e.touches[0].clientX); }, { passive: true });
		hit.addEventListener('touchmove', function(e) { if (e.touches[0]) show(e.touches[0].clientX); }, { passive: true });
	}

	function column(h, field)
	{
		var idx = h.Fields.indexOf(field);
		return h.Samples.map(function(row) { return row[idx]; });
	}

	this.chartSpecs = function(d)
	{
		var h = d.History || { Fields: [], Samples: [], Interval: 5 };
		var times = column(h, 'Time');
		var base = { times: times, interval: h.Interval || 5, range: range };
		return [
			$.extend({ id: 'SystemChart_Speed', title: 'Download speed, all servers', fmt: speed, bytes: true,
				series: [{ name: 'Speed', cls: 's1', values: column(h, 'Speed') }] }, base),
			$.extend({ id: 'SystemChart_Connections', title: 'Connections in use', fmt: function(v) { return Math.round(v) + ''; },
				series: [{ name: 'Connections', cls: 's1', values: column(h, 'Connections') }] }, base),
			$.extend({ id: 'SystemChart_Load', title: 'Load average', fmt: load,
				series: [
					{ name: '1 min', cls: 's1', values: column(h, 'Load1') },
					{ name: '5 min', cls: 's2', values: column(h, 'Load5') },
					{ name: '15 min', cls: 's3', values: column(h, 'Load15') }] }, base),
			$.extend({ id: 'SystemChart_Cpu', title: 'CPU, percent of all ' + (d.CpuCount || '') + ' CPUs', fmt: pct, max: 100,
				series: [
					{ name: 'Machine', cls: 's1', values: column(h, 'Cpu') },
					{ name: 'RustyGet', cls: 's2', values: column(h, 'ProcessCpu') }] }, base),
			$.extend({ id: 'SystemChart_Ram', title: 'RAM in use, of ' + bytes((d.Memory || {}).Total), fmt: bytes, bytes: true,
				max: (d.Memory || {}).Total || 0,
				series: [
					{ name: 'Machine', cls: 's1', values: column(h, 'MemUsed') },
					{ name: 'RustyGet', cls: 's2', values: column(h, 'ProcessMem') }] }, base)
		];
	};

	function renderCharts($el, d)
	{
		if (!$el || !$el.length) return;
		$el.empty();
		SystemMetrics.chartSpecs(d).forEach(function(spec) { $el.append(SystemMetrics.chart(spec)); });
		redrawCharts();
	}

	function redrawCharts()
	{
		$('#SystemStats_Charts .sysmetrics-chart:visible').each(function() { drawPlot($(this)); });
	}

	// ---- wiring ----

	function statusVisible() { return $('#SysInfo_Machine').parent().is(':visible'); }
	function statsVisible() { return $('#SystemStats').is(':visible'); }

	function render(d, force)
	{
		last = d;
		if (statusVisible()) renderTiles($('#SysInfo_Machine'), d);
		if (statsVisible())
		{
			renderTiles($('#SystemStats_Tiles'), d);
			// a poll doesn't rebuild the chart under the reader's pointer
			if (!hovering || force) renderCharts($('#SystemStats_Charts'), d);
		}
	}

	var pending = false;

	this.update = function(force)
	{
		if (loading)
		{
			// a page just became visible while a poll is out: fetch again after it
			pending = true;
			return;
		}
		if (document.hidden && !force) return;
		var wantStats = statsVisible();
		if (!force && !wantStats && !statusVisible()) return;
		loading = true;
		var done = function()
		{
			loading = false;
			if (pending)
			{
				pending = false;
				SystemMetrics.update(false);
			}
		};
		RPC.call('systemstats', [wantStats ? range : 60], function(d)
		{
			render(d, force);
			done();
		}, done);
	};

	this.redraw = function()
	{
		if (last) render(last, true);
	};

	this.setRange = function(sec, quiet)
	{
		range = sec;
		$('#SystemStats_Range button').each(function()
		{
			var on = +$(this).data('range') === sec;
			$(this).toggleClass('btn-active', on).attr('aria-pressed', on ? 'true' : 'false');
		});
		if (!quiet) this.update(true);
	};

	this.getLast = function() { return last; };

	this.init = function()
	{
		var $range = $('#SystemStats_Range').empty();
		RANGES.forEach(function(r)
		{
			$range.append($('<button type="button" class="btn">').text(r.label).attr('data-range', r.sec)
				.on('click', function() { SystemMetrics.setRange(r.sec); }));
		});
		this.setRange(range, true);
		$(window).on('resize', function() { clearTimeout(SystemMetrics._rz); SystemMetrics._rz = setTimeout(redrawCharts, 150); });
		$(document).on('shown', 'a[data-toggle="tab"]', function() { setTimeout(function() { SystemMetrics.update(false); }, 0); });
		if (!timer) timer = setInterval(function() { SystemMetrics.update(false); }, POLL_MS);
	};
}(jQuery));
