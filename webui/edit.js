/*
 * This file is part of nzbget. See <https://nzbget.com>.
 *
 * Copyright (C) 2012-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 * Copyright (C) 2026 Denis <denis@nzbget.com>
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
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

/*
 * In this module:
 *   1) Download edit dialog;
 *   2) Download multi edit dialog (edit multiple items);
 *   3) Download merge dialog;
 *   4) Download split dialog;
 *   5) History edit dialog;
 *   6) Details panel: summary tiles and dupe fleet shared by 1) and 5).
 */

/*** DETAILS PANEL: SUMMARY TILES AND DUPE FLEET ************************************/

// The edit dialogs of a download and of a history item open as a side panel next
// to the list. This module renders what both share: the headline numbers as
// tiles (replacing the tall statistics table) and the item's dupe fleet - every
// copy of the release in the queue and in history, its role (primary, twin,
// alt), health and status - with each copy one click away.
var DetailsPanel = (new function($)
{
	'use strict';

	// donor priority, lower first: the primary, twins, then borrowable alts,
	// encrypted alts borrowable with their nzb password, alts whose packaging
	// is unknown, and alts that cannot be borrowed from
	var ROLE_ORDER = { primary: 0, twin: 1, borrowable: 2, 'borrowable-encrypted': 3, alt: 4, container: 5,
		unborrowable: 6, '': 7 };

	function param(item, name)
	{
		return DownloadsUI.parameterValue(item.Parameters, name);
	}

	// role of a copy within its fleet: { kind, label, copy } or null
	this.role = function(item)
	{
		if (item.Kind !== 'NZB')
		{
			return null;
		}
		var kind = param(item, 'DupeKind');
		var copy = param(item, 'DupeCopyKind');
		var copyLabel = copy === 'borrowable' ? I18n.translate('fleet_copy_borrowable') :
			copy === 'borrowable-encrypted' ? I18n.translate('fleet_copy_borrowable_encrypted') :
			copy === 'unborrowable' ? I18n.translate('fleet_copy_unborrowable') : '';
		var near = /^twin:(\d+\/\d+)$/.exec(kind);
		if (kind === 'twin' || near)
		{
			return { kind: 'twin', label: I18n.translate('fleet_role_twin') + (near ? ' ' + near[1] : ''), copy: '' };
		}
		if (kind === 'alt')
		{
			return { kind: copy === 'borrowable' || copy === 'borrowable-encrypted' ||
				copy === 'unborrowable' ? copy : 'alt',
				label: I18n.translate('fleet_role_alt'), copy: copyLabel };
		}
		// another container whose video inside is not verified yet: shown as
		// unverified (other par2 checksums only mean other packing), it becomes
		// an alt once the video inside matches
		if (DupeGroup.kindLabel(kind) === 'unverified')
		{
			return { kind: 'container', label: I18n.translate('fleet_role_unverified'), copy: copyLabel };
		}
		// another video: a plain copy, never the primary
		if (kind === 'other')
		{
			return null;
		}
		if (/^[0-9a-f]{16}-[0-9]+$/.test(param(item, 'DupeFiles')))
		{
			// a primary with twins in its fleet is a twin too
			return { kind: 'primary', label: I18n.translate('fleet_role_primary'),
				copy: copy === 'twin' ? I18n.translate('fleet_role_twin') : '' };
		}
		return null;
	}

	// health in permille: the live value once articles were fetched, otherwise
	// the health check's sample of a copy that has not started. DupeHealth and
	// DupeAlive hold percentages, fractions allowed (as parse_alive_permille_bytes
	// in rust/src/dupecoord.rs reads them).
	function healthPermille(item)
	{
		if (item.SuccessArticles + item.FailedArticles > 0)
		{
			return item.Health;
		}
		var best = -1;
		var keys = ['DupeHealth', 'DupeAlive'];
		for (var i = 0; i < keys.length; i++)
		{
			var percent = parseFloat(param(item, keys[i]));
			if (!isNaN(percent))
			{
				best = Math.max(best, Math.round(Math.min(Math.max(percent, 0), 100) * 10));
			}
		}
		return best >= 0 ? best : item.Health;
	}

	// below the critical health a copy cannot be repaired, whatever its value
	function healthClass(permille, critical)
	{
		return permille < (critical || 850) ? 'is-bad' : permille >= 950 ? 'is-good' : 'is-warn';
	}

	function formatPercent(permille)
	{
		var percent = Math.floor(permille) / 10;
		return (percent === 100 ? '100' : percent.toFixed(1)) + '%';
	}

	function tile(label, value, sub, valueClass, link)
	{
		var open = link ? '<a href="#" class="details-tile details-tile-link" id="' + link.id + '" data-tab="' + link.tab +
			'" title="' + Util.textToAttr(link.title) + '">' : '<div class="details-tile">';
		return open +
			'<span class="details-tile-label">' + Util.textToHtml(label) +
			(link ? ' <i class="material-icon" aria-hidden="true">arrow_right_alt</i>' : '') + '</span>' +
			'<span class="details-tile-value ' + (valueClass || '') + '">' + Util.textToHtml(value) + '</span>' +
			'<span class="details-tile-sub">' + Util.textToHtml(sub) + '</span>' +
			(link ? '</a>' : '</div>');
	}

	function completion(item)
	{
		var done = item.SuccessArticles + item.FailedArticles;
		if (done === 0)
		{
			return '--';
		}
		var value = Util.round0(item.SuccessArticles * 100.0 / done);
		return (item.FailedArticles > 0 && value == 100 ? '99.9' : value) + '%';
	}

	// headline numbers of a queued download (prefix 'DownloadsEdit') or of a
	// history item (prefix 'HistoryEdit')
	this.summary = function(item, prefix)
	{
		var history = prefix === 'HistoryEdit';
		var html = '';
		var sub;
		if (history)
		{
			sub = I18n.translate('details_total_time', Util.formatTimeHMS(item.DownloadTimeSec + item.PostTotalTimeSec));
			html += tile(I18n.translate('label_total'), Util.formatSizeMB(item.FileSizeMB, item.FileSizeLo), sub, '',
				{ id: 'HistoryEdit_TimeStats', tab: 'HistoryEdit_TimeStatsTab', title: I18n.translate('tooltip_history_total_stats') });
			html += tile(I18n.translate('details_files'), String(item.FileCount),
				I18n.translate('details_left', String(item.RemainingFileCount)));
		}
		else
		{
			var left = Util.formatSizeMB(item.RemainingSizeMB - item.PausedSizeMB, item.RemainingSizeLo - item.PausedSizeLo);
			sub = I18n.translate('details_left', left);
			if (item.PausedSizeMB > 0 || item.PausedSizeLo > 0)
			{
				sub += ' · ' + I18n.translate('details_paused', Util.formatSizeMB(item.PausedSizeMB, item.PausedSizeLo));
			}
			html += tile(I18n.translate('label_total'), Util.formatSizeMB(item.FileSizeMB, item.FileSizeLo), sub);
			html += tile(I18n.translate('details_files'), String(item.FileCount),
				I18n.translate('details_files_left', String(item.RemainingFileCount), String(item.RemainingParCount)));
		}
		html += tile(I18n.translate('details_health'), formatPercent(item.Health),
			I18n.translate('details_critical', formatPercent(item.CriticalHealth)), healthClass(item.Health, item.CriticalHealth));
		html += tile(I18n.translate('details_articles'), String(item.TotalArticles),
			I18n.translate('details_complete', completion(item)), '',
			item.ServerStats.length > 0 ? { id: prefix + '_ServStats', tab: prefix + '_ServStatsTab',
				title: I18n.translate(history ? 'tooltip_history_server_stats' : 'tooltip_per_server_stats') } : null);

		var bar = '';
		if (!history && item.FileSizeMB > 0)
		{
			var percent = Math.max(0, Math.min(100, Math.round((item.FileSizeMB - item.RemainingSizeMB) * 100 / item.FileSizeMB)));
			bar = '<div class="details-progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="' + percent +
				'"><span style="width:' + percent + '%"></span></div>';
		}
		return '<div class="details-tiles">' + html + '</div>' + bar;
	}

	// every copy sharing the item's dupe key, the queue first, in donor priority
	this.members = function(item)
	{
		var members = [];
		if (!item.DupeKey)
		{
			return members;
		}
		// the daemon matches dupe keys ignoring ASCII case (eq_ignore_ascii_case)
		var key = item.DupeKey.toLowerCase();
		function collect(list, source)
		{
			for (var i = 0; list && i < list.length; i++)
			{
				var other = list[i];
				if (other.Kind === 'NZB' && other.DupeKey.toLowerCase() === key)
				{
					members.push({ item: other, source: source, role: DetailsPanel.role(other) });
				}
			}
		}
		collect(Downloads.groups, 'queue');
		collect(History.items, 'history');
		members.sort(function(a, b)
		{
			if (a.source !== b.source)
			{
				return a.source === 'queue' ? -1 : 1;
			}
			var ra = ROLE_ORDER[a.role ? a.role.kind : ''], rb = ROLE_ORDER[b.role ? b.role.kind : ''];
			return ra !== rb ? ra - rb : healthPermille(b.item) - healthPermille(a.item);
		});
		return members;
	}

	function roleTag(role, withCopy)
	{
		if (!role)
		{
			return '<span class="fleet-role fleet-role-none">' + Util.textToHtml(I18n.translate('fleet_role_copy')) + '</span>';
		}
		return '<span class="fleet-role fleet-role-' + role.kind + '">' + Util.textToHtml(role.label) + '</span>' +
			(withCopy && role.copy ? ' <span class="fleet-copy">' + Util.textToHtml(role.copy) + '</span>' : '');
	}

	// length of the prefix all names share, cut back to a word start; 0 when
	// it is too short to be worth hiding
	function commonPrefixAtWord(names)
	{
		if (names.length < 2)
		{
			return 0;
		}
		var len = names[0].length;
		for (var i = 1; i < names.length; i++)
		{
			var k = 0;
			while (k < len && k < names[i].length && names[i][k] === names[0][k])
			{
				k++;
			}
			len = k;
		}
		var cut = names[0].lastIndexOf(' ', len) + 1;
		return cut >= 16 ? cut : 0;
	}

	function countParam(item, name)
	{
		var n = parseInt(param(item, name), 10);
		return isNaN(n) || n < 0 ? 0 : n;
	}

	function stat(cls, label, value, sub)
	{
		return '<div class="fleet-stat ' + cls + '"><span class="fleet-stat-label">' + Util.textToHtml(label) + '</span>' +
			'<span class="fleet-stat-value">' + Util.textToHtml(value) + '</span>' +
			'<span class="fleet-stat-sub">' + Util.textToHtml(sub) + '</span></div>';
	}

	// the other copies this one can borrow from: twins (the same post, swapped
	// article by article) and borrowable alts. A twin is the same post, so it holds
	// the same video; an alt counts as verified once the fleet intake found the
	// fleet's video in it (DupeVideo=same, set by rust/src/dupesearch_coord.rs).
	this.donors = function(item, members, source)
	{
		var out = { twins: 0, alts: 0, unverified: 0 };
		// one post is one donor: a history copy of a post already counted (or of
		// this copy's own post) adds nothing. Members come queue first.
		// Names are what the client sent, so two queued copies may share one, but
		// the intake never queues one post twice: a name hides a copy only when
		// the copy or the one that claimed the name is in history.
		var posts = {};
		posts[item.NZBFilename] = source;
		// prints of this fleet's own release: the viewed copy's, and those of the
		// primary and twins in the queue. Any copy can carry a print, so a history
		// copy marked primary or twin (say an older release under the same dupe key,
		// or an old fleet's copy) is only this release when its print is one of
		// these. With nothing of the release in the queue, there is nothing left
		// downloading to borrow into, so history copies seen from an alt add none.
		// Only real prints anchor: a twin whose par2 set was missing or lost carries
		// a placeholder (none, lost), and counts only while it is in the queue.
		var prints = {};
		function realPrint(it)
		{
			var p = param(it, 'DupeFiles');
			return /^[0-9a-f]{16}-[0-9]+$/.test(p) ? p : null;
		}
		var selfRole = DetailsPanel.role(item);
		if (selfRole && (selfRole.kind === 'twin' || selfRole.kind === 'primary') && realPrint(item))
		{
			prints[realPrint(item)] = true;
		}
		for (var j = 0; j < members.length; j++)
		{
			var r = members[j].role ? members[j].role.kind : '';
			if ((r === 'twin' || r === 'primary') && members[j].source === 'queue' && realPrint(members[j].item))
			{
				prints[realPrint(members[j].item)] = true;
			}
		}
		var anchored = Object.keys(prints).length > 0;
		for (var i = 0; i < members.length; i++)
		{
			var m = members[i];
			var claimed = posts[m.item.NZBFilename];
			if ((m.item.NZBID === item.NZBID && m.source === source) ||
				(claimed && (claimed === 'history' || m.source === 'history')))
			{
				continue;
			}
			// a dead copy still claims its post, so its history tries add nothing
			posts[m.item.NZBFilename] = claimed || m.source;
			if (healthPermille(m.item) <= 0)
			{
				continue;
			}
			var kind = m.role ? m.role.kind : '';
			// the primary and its twins are one release posted more than once:
			// counted the same way whichever copy is viewed
			var print = realPrint(m.item);
			// a near twin (twin:N/M) holds other files, so its print never matches;
			// its kind is resolved against the current primary, so it counts while
			// the release is in the queue, like a twin without a print
			var near = /^twin:\d+\/\d+$/.test(param(m.item, 'DupeKind'));
			if ((kind === 'twin' || kind === 'primary') &&
				((print && prints[print]) ||
				(kind === 'twin' && (m.source === 'queue' || (near && anchored)))))
			{
				out.twins++;
			}
			else if (kind !== 'primary' && kind !== 'twin' && /^borrowable(-encrypted)?$/.test(param(m.item, 'DupeCopyKind')) &&
				param(m.item, 'DupeVideo') !== 'different')
			{
				if (param(m.item, 'DupeVideo') === 'same')
				{
					out.alts++;
				}
				else
				{
					out.unverified++;
				}
			}
		}
		return out;
	}

	// what this copy borrowed from the others, and what it still can
	function borrowStats(item, selfRole, members, source)
	{
		var articles = item.DupeRecoveredArticles || 0;
		var parArticles = countParam(item, 'DupeParArticles');
		// DupeParFiles lists the par2 files still holding borrowed articles, as id:count
		var parFiles = (param(item, 'DupeParFiles') || '').split(',').filter(function(f) {
			var m = /^\s*\d+\s*(?::\s*(\d+)\s*)?$/.exec(f);
			return m && (m[1] === undefined || parseInt(m[1], 10) > 0);
		}).length;
		var hi = item.DupeRecoveredBytesHi || 0, lo = item.DupeRecoveredBytesLo || 0;
		var holes = item.DupeRecoveredHoles || 0;
		var donors = DetailsPanel.donors(item, members, source);
		var verified = donors.twins + donors.alts;
		var split = I18n.translate('fleet_donor_split', String(donors.twins), String(donors.alts));
		if (donors.unverified > 0)
		{
			split += ' · ' + I18n.translate('fleet_donor_unverified', String(donors.unverified));
		}
		return '<div class="fleet-stats">' +
			stat('fleet-stat-articles', I18n.translate('fleet_stat_articles'), String(articles), I18n.translate('fleet_stat_articles_sub')) +
			stat('fleet-stat-par', I18n.translate('fleet_stat_par'), String(parArticles), parArticles > 0 ?
				I18n.translate(parFiles === 1 ? 'fleet_stat_par_file' : 'fleet_stat_par_files', String(parFiles)) :
				I18n.translate('fleet_stat_articles_sub')) +
			stat('fleet-stat-bytes', I18n.translate('fleet_stat_bytes'), Util.formatSizeMB(hi * 4096 + lo / 1024.0 / 1024.0, lo),
				I18n.translate('fleet_holes', String(holes))) +
			stat('fleet-stat-donors', I18n.translate('fleet_stat_donors'), I18n.translate('fleet_donor_verified', String(verified)), split) +
			'</div>';
	}

	// the fleet card; empty when the item has no other copy and nothing was
	// recovered from duplicates
	this.fleet = function(item, source)
	{
		var members = this.members(item);
		var recovered = (item.DupeRecoveredArticles || 0) > 0 || (item.DupeRecoveredBytesLo || 0) > 0 || (item.DupeRecoveredBytesHi || 0) > 0;
		if (members.length < 2 && !recovered && !item.LiveRepairing)
		{
			return '';
		}

		var facts = [];
		var selfRole = this.role(item);
		facts.push('<span class="fleet-fact">' + roleTag(selfRole, true) + '</span>');
		if (item.LiveRepairing)
		{
			facts.push('<span class="fleet-fact fleet-live" title="' + Util.textToAttr(I18n.translate('label_live_repair_hint')) + '">' +
				'<i class="material-icon" aria-hidden="true">build</i> ' + Util.textToHtml(I18n.translate('fleet_live_repair')) + '</span>');
		}

		// copies of a release share most of their name: show where they differ
		var names = [];
		for (var j = 0; j < members.length; j++)
		{
			names.push(Util.formatNZBName(members[j].item.NZBName));
		}
		var common = commonPrefixAtWord(names);

		var rows = '';
		for (var i = 0; i < members.length; i++)
		{
			var m = members[i];
			var self = m.item.NZBID === item.NZBID && m.source === source;
			var health = healthPermille(m.item);
			var status = m.source === 'queue' ? DownloadsUI.buildStatusText(m.item) : HistoryUI.buildStatusText(m.item);
			var name = names[i];
			var shown = common > 0 && name.length > common ? '\u2026' + name.substring(common) : name;
			var cells = '<span class="fleet-member-role">' + roleTag(m.role) + '</span>' +
				'<span class="fleet-member-name" title="' + Util.textToAttr(name) + '">' + Util.textToHtml(shown) + '</span>' +
				'<span class="fleet-member-health ' + healthClass(health, m.item.CriticalHealth) + '">' + formatPercent(health) + '</span>' +
				'<span class="fleet-member-status">' + Util.textToHtml(status) +
				(m.role && m.role.copy ? ' · ' + Util.textToHtml(m.role.copy) : '') +
				(m.source === 'history' ? ' · ' + Util.textToHtml(I18n.translate('fleet_in_history')) : '') + '</span>';
			rows += self ?
				'<li class="fleet-member is-current" aria-current="true">' + cells + '</li>' :
				'<li><a href="#" class="fleet-member" data-fleet-source="' + m.source + '" data-fleet-id="' + m.item.NZBID + '">' + cells + '</a></li>';
		}

		var stats = members.length > 1 || recovered || Util.showDupeRecovered(item) ? borrowStats(item, selfRole, members, source) : '';

		return '<div class="fleet-head"><span class="fleet-title">' + Util.textToHtml(I18n.translate('fleet_title')) + '</span>' +
			(item.DupeKey ? '<code class="fleet-key" title="' + Util.textToAttr(I18n.translate('edit_dupe_key')) + '">' + Util.textToHtml(item.DupeKey) + '</code>' : '') +
			'<span class="fleet-facts">' + facts.join('') + '</span>' +
			(members.length > 1 ? '<span class="fleet-count">' + Util.textToHtml(I18n.translate('fleet_copies', String(members.length))) + '</span>' : '') +
			'</div>' +
			stats +
			(members.length > 1 ? '<ul class="fleet-members">' + rows + '</ul>' : '');
	}

	// switch the panel to another copy of the fleet
	this.open = function(source, id)
	{
		if (source === 'queue')
		{
			if ($('#HistoryEditDialog').hasClass('in'))
			{
				$('#HistoryEditDialog').modal('hide');
			}
			DownloadsEditDialog.showModal(id, Downloads.groups);
		}
		else
		{
			for (var i = 0; History.items && i < History.items.length; i++)
			{
				if (History.items[i].NZBID == id)
				{
					if ($('#DownloadsEditDialog').hasClass('in'))
					{
						$('#DownloadsEditDialog').modal('hide');
					}
					HistoryEditDialog.showModal(History.items[i]);
					return;
				}
			}
		}
	}

	// The backdrop stays 'static' so Bootstrap never closes the panel on its own,
	// mid-save included: a click beside the panel closes it here instead, unless
	// a save is still in flight
	this.closeOnOutsideClick = function($dialog)
	{
		var $backdrop = $('.modal-backdrop').last();
		$backdrop.off('click.details').on('click.details', function()
		{
			// a confirm opened from the panel shares this backdrop: leave both open
			if ($dialog.hasClass('in') && !$dialog.hasClass('is-saving') && !$('.modal.in:visible').not($dialog).length)
			{
				$dialog.modal('hide');
			}
		});
	}

	this.bindFleet = function($container)
	{
		$container.on('click', 'a.fleet-member', function(e)
		{
			e.preventDefault();
			// a save in flight sends its remaining edits to the shown copy:
			// stay on it until the save completes
			if ($container.closest('.modal').hasClass('is-saving'))
			{
				return;
			}
			DetailsPanel.open($(this).attr('data-fleet-source'), parseInt($(this).attr('data-fleet-id'), 10));
		});
	}
}(jQuery));


/*** DOWNLOAD EDIT DIALOG ************************************************************/

var DownloadsEditDialog = (new function($)
{
	'use strict';

	// Controls
	var $DownloadsEditDialog;
	var $DownloadsFileTable;
	var $DownloadsEdit_ParamData;
	var $ServStatsTable;

	// State
	var curGroup;
	var notification = null;
	var postParams = [];
	var lastPage;
	var lastFullscreen;
	var logFilled;
	var files;
	var refreshTimer = 0;
	var showing;
	var oldCategory;

	this.init = function()
	{
		$DownloadsEditDialog = $('#DownloadsEditDialog');
		$DownloadsEdit_ParamData = $('#DownloadsEdit_ParamData');

		$('#DownloadsEdit_Save').click(saveChanges);
		$('#DownloadsEdit_Actions').click(itemActions);
		$('#DownloadsEdit_Param, #DownloadsEdit_Log, #DownloadsEdit_File, #DownloadsEdit_Dupe').click(tabClick);
		$('#DownloadsEdit_Back').click(backClick);
		$('#DownloadsEdit_Category').change(categoryChange);

		LogTab.init('Downloads');

		$DownloadsFileTable = $('#DownloadsEdit_FileTable');
		$DownloadsFileTable.fasttable(
			{
				filterInput: '#DownloadsEdit_FileTable_filter',
				pagerContainer: '#DownloadsEdit_FileTable_pager',
				rowSelect: UISettings.rowSelect,
				pageSize: 10000,
				renderCellCallback: fileTableRenderCellCallback
			});

		$ServStatsTable = $('#DownloadsEdit_ServStatsTable');
		$ServStatsTable.fasttable(
			{
				filterInput: '#DownloadsEdit_ServStatsTable_filter',
				pagerContainer: '#DownloadsEdit_ServStatsTable_pager',
				pageSize: 100,
				maxPages: 3,
				renderCellCallback: EditUI.servStatsTableRenderCellCallback
			});

		$DownloadsEditDialog.on('hidden', function()
		{
			// cleanup
			LogTab.reset('Downloads');
			$DownloadsFileTable.fasttable('update', []);
			$DownloadsEdit_ParamData.empty();
			clearTimeout(refreshTimer);
			// resume updates
			Refresher.resume();
		});

		TabDialog.extend($DownloadsEditDialog);
		DetailsPanel.bindFleet($('#DownloadsEdit_Fleet'));

		if (UISettings.setFocus)
		{
			$DownloadsEditDialog.on('shown', function()
			{
				if ($('#DownloadsEdit_NZBName').is(":visible"))
				{
					$('#DownloadsEdit_NZBName').focus();
				}
				if ($DownloadsEdit_ParamData && $DownloadsEdit_ParamData.children().length > 0)
				{
					ParamTab.updatePostParamTranslations();
				}
			});
		}
		else
		{
			$DownloadsEditDialog.on('shown', function()
			{
				if ($DownloadsEdit_ParamData && $DownloadsEdit_ParamData.children().length > 0)
				{
					ParamTab.updatePostParamTranslations();
				}
			});
		}

		I18n.subscribe(function()
		{
			if ($DownloadsEdit_ParamData && $DownloadsEdit_ParamData.children().length > 0)
			{
				ParamTab.updatePostParamTranslations();
			}
		});
	}

	this.showModal = function(nzbid, allGroups, area)
	{
		var group = null;

		// find Group object
		for (var i=0; i<allGroups.length; i++)
		{
			var gr = allGroups[i];
			if (gr.NZBID == nzbid)
			{
				group = gr;
				break;
			}
		}
		if (group == null)
		{
			return;
		}

		// the panel is already open on another copy of the fleet: switch over
		var switching = $DownloadsEditDialog.hasClass('in');
		if (!switching)
		{
			Refresher.pause();
		}

		curGroup = group;

		$('#DownloadsEdit_Status').html(DownloadsUI.buildStatus(group));
		$('#DownloadsEdit_Summary').html(group.Kind === 'NZB' ? DetailsPanel.summary(group, 'DownloadsEdit') : '');
		$('#DownloadsEdit_Fleet').html(group.Kind === 'NZB' ? DetailsPanel.fleet(group, 'queue') : '');
		$('#DownloadsEdit_ServStats').click(tabClick);
		EditUI.fillServStats($ServStatsTable, group);
		$ServStatsTable.fasttable('setCurPage', 1);

		$('#DownloadsEdit_Title').html(Util.textToHtml(Util.formatNZBName(group.NZBName)) +
		    (group.Kind === 'URL' ? '&nbsp;<span class="label label-info text-uppercase">' + I18n.translate('label_kind_url') + '</span>' : ''));

		$('#DownloadsEdit_NZBName').attr('value', group.NZBName);
		$('#DownloadsEdit_NZBName').attr('readonly', group.postprocess);
		$('#DownloadsEdit_URL').attr('value', group.URL);

		// Priority
		var v = $('#DownloadsEdit_Priority');
		DownloadsUI.fillPriorityCombo(v);
		v.val(group.MaxPriority);
		if (v.val() != group.MaxPriority)
		{
			v.append('<option selected="selected">' + group.MaxPriority +'</option>');
		}
		v.attr('disabled', 'disabled');

		// Category
		v = $('#DownloadsEdit_Category');
		DownloadsUI.fillCategoryCombo(v);
		v.val(group.Category);
		if (v.val() != group.Category)
		{
			v.append($('<option selected="selected"></option>').text(group.Category));
		}

		// duplicate settings
		$('#DownloadsEdit_DupeKey').val(group.DupeKey);
		$('#DownloadsEdit_DupeScore').val(group.DupeScore);
		$('#DownloadsEdit_DupeMode').val(group.DupeMode);

		$DownloadsFileTable.fasttable('update', []);

		var postParamConfig = ParamTab.createPostParamConfig();

		Util.show('#DownloadsEdit_NZBNameReadonly', group.postprocess);
		Util.show('#DownloadsEdit_Save', !group.postprocess);
		Util.show('#DownloadsEdit_Summary, #DownloadsEdit_Fleet', group.Kind === 'NZB');
		Util.show('#DownloadsEdit_File', group.Kind === 'NZB');
		Util.show('#DownloadsEdit_URLGroup', group.Kind === 'URL');
		$('#DownloadsEdit_CategoryGroup').toggleClass('control-group-last', group.Kind === 'URL');
		var dupeCheck = Options.option('DupeCheck') === 'yes';
		Util.show('#DownloadsEdit_Dupe', dupeCheck);
		var postParam = postParamConfig[0].options.length > 0 && group.Kind === 'NZB';
		// every download has a log, from the moment it is queued
		var postLog = group.Kind === 'NZB' || group.MessageCount > 0;
		Util.show('#DownloadsEdit_Param', postParam);
		Util.show('#DownloadsEdit_Log', postLog);

		if (group.postprocess)
		{
			$('#DownloadsEdit_NZBName').attr('disabled', 'disabled');
			$('#DownloadsEdit_Priority').attr('disabled', 'disabled');
			$('#DownloadsEdit_Category').attr('disabled', 'disabled');
			$('#DownloadsEdit_Close').addClass('btn-primary');
			$('#DownloadsEdit_Close').text(I18n.translate('btn_close'));
		}
		else
		{
			$('#DownloadsEdit_NZBName').removeAttr('disabled');
			$('#DownloadsEdit_Priority').removeAttr('disabled');
			$('#DownloadsEdit_Category').removeAttr('disabled');
			$('#DownloadsEdit_Close').removeClass('btn-primary');
			$('#DownloadsEdit_Close').text(I18n.translate('btn_cancel'));
		}

		if (postParam)
		{
			postParams = ParamTab.buildPostParamTab($DownloadsEdit_ParamData, postParamConfig, curGroup.Parameters);
			ParamTab.updatePostParamTranslations();
		}

		enableAllButtons();

		$('#DownloadsEdit_GeneralTab').show();
		$('#DownloadsEdit_ParamTab').hide();
		$('#DownloadsEdit_ServStatsTab').hide();
		$('#DownloadsEdit_LogTab').hide();
		$('#DownloadsEdit_FileTab').hide();
		$('#DownloadsEdit_DupeTab').hide();
		$('#DownloadsEdit_Back').hide();
		$('#DownloadsEdit_BackSpace').show();
		$DownloadsEditDialog.restoreTab();

		$('#DownloadsEdit_FileTable_filter').val('');
		$DownloadsFileTable.fasttable('setCurPage', 1);
		$DownloadsFileTable.fasttable('applyFilter', '');

		LogTab.reset('Downloads');

		files = null;
		logFilled = false;
		notification = null;
		oldCategory = curGroup.Category;

		if (area === 'backup')
		{
			showing = true;
			$('#DownloadsEdit_ServStats').trigger('click');
		}
		showing = false;

		if (!switching)
		{
			$DownloadsEditDialog.modal({backdrop: 'static'});
			DetailsPanel.closeOnOutsideClick($DownloadsEditDialog);
		}
	}

	function completed()
	{
		$DownloadsEditDialog.modal('hide');
		Refresher.update();
		if (notification)
		{
			PopupNotification.show(notification);
			notification = null;
		}
	}

	function tabClick(e)
	{
		e.preventDefault();

		$('#DownloadsEdit_Back').fadeIn(showing ? 0 : 500);
		$('#DownloadsEdit_BackSpace').hide();
		var tab = '#' + $(this).attr('data-tab');
		lastPage = $(tab);
		lastFullscreen = ($(this).attr('data-fullscreen') === 'true') && !UISettings.miniTheme;

		$('#DownloadsEdit_FileBlock').removeClass('modal-inner-scroll');
		$('#DownloadsEdit_FileBlock').css('top', '');

		if (UISettings.miniTheme && files === null)
		{
			$('#DownloadsEdit_FileBlock').css('min-height', $DownloadsEditDialog.height());
		}

		if (UISettings.miniTheme && !logFilled)
		{
			$('#DownloadsEdit_LogBlock').css('min-height', $DownloadsEditDialog.height());
		}

		$DownloadsEditDialog.switchTab($('#DownloadsEdit_GeneralTab'), lastPage,
			e.shiftKey || !UISettings.slideAnimation || showing ? 0 : 500,
			{fullscreen: lastFullscreen, mini: UISettings.miniTheme, complete: function()
				{
					if (!UISettings.miniTheme)
					{
						$('#DownloadsEdit_FileBlock').css('top', $('#DownloadsEdit_FileBlock').position().top);
						$('#DownloadsEdit_FileBlock').addClass('modal-inner-scroll');
					}
					else
					{
						$('#DownloadsEdit_FileBlock').css('min-height', '');
						$('#DownloadsEdit_LogBlock').css('min-height', '');
					}
				}});

		if (tab === '#DownloadsEdit_LogTab' && !logFilled)
		{
			LogTab.fill('Downloads', curGroup);
			logFilled = true;
		}

		if (tab === '#DownloadsEdit_FileTab' && files === null)
		{
			fillFiles();
		}

		if (tab === '#DownloadsEdit_ServStatsTab')
		{
			scheduleRefresh();
		}
	}

	function backClick(e)
	{
		e.preventDefault();
		$('#DownloadsEdit_Back').fadeOut(500, function()
		{
			$('#DownloadsEdit_BackSpace').show();
		});

		$('#DownloadsEdit_FileBlock').removeClass('modal-inner-scroll');
		$('#DownloadsEdit_FileBlock').css('top', '');

		$DownloadsEditDialog.switchTab(lastPage, $('#DownloadsEdit_GeneralTab'),
			e.shiftKey || !UISettings.slideAnimation ? 0 : 500,
			{fullscreen: lastFullscreen, mini: UISettings.miniTheme, back: true});

		clearTimeout(refreshTimer);
	}

	function disableAllButtons()
	{
		$('#DownloadsEditDialog .modal-footer .btn').attr('disabled', 'disabled');
		$('#DownloadsEditDialog').addClass('is-saving');
		setTimeout(function()
		{
			$('#DownloadsEdit_Transmit').show();
		}, 500);
	}

	function enableAllButtons()
	{
		$('#DownloadsEditDialog .modal-footer .btn').removeAttr('disabled');
		$('#DownloadsEditDialog').removeClass('is-saving');
		$('#DownloadsEdit_Transmit').hide();
	}

	function saveChanges(e)
	{
		e.preventDefault();
		disableAllButtons();
		notification = null;
		saveName();
	}

	function saveName()
	{
		var name = $('#DownloadsEdit_NZBName').val();
		name !== curGroup.NZBName && !curGroup.postprocess ?
			RPC.call('editqueue', ['GroupSetName', name, [curGroup.NZBID]], function()
			{
				notification = '#Notif_Downloads_Saved';
				savePriority();
			})
			:savePriority();
	}

	function savePriority()
	{
		var priority = parseInt($('#DownloadsEdit_Priority').val());
		priority !== curGroup.MaxPriority ?
			RPC.call('editqueue', ['GroupSetPriority', '' + priority, [curGroup.NZBID]], function()
			{
				notification = '#Notif_Downloads_Saved';
				saveCategory();
			})
			: saveCategory();
	}

	function saveCategory()
	{
		var category = $('#DownloadsEdit_Category').val();
		category !== curGroup.Category ?
			RPC.call('editqueue', ['GroupSetCategory', category, [curGroup.NZBID]], function()
			{
				notification = '#Notif_Downloads_Saved';
				saveDupeKey();
			})
			: saveDupeKey();
	}

	function itemActions(e)
	{
		e.preventDefault();
		e.stopPropagation();
		var elem = $('#DownloadsEdit_Actions').parent();

		DownloadsActionsMenu.showPopupMenu(curGroup, 'top',
			{ left: elem.offset().left, top: elem.offset().top - 1,
				width: elem.width(), height: elem.height() + 2 },
			function(_notification)
			{
				disableAllButtons();
				notification = _notification;
			},
			completed);
	}

	function categoryChange()
	{
		var category = $('#DownloadsEdit_Category').val();
		ParamTab.reassignParams(postParams, oldCategory, category);
		oldCategory = category;
	}

	/*** TAB: POST-PROCESSING PARAMETERS **************************************************/

	function saveParam()
	{
		if (curGroup.Kind === 'URL')
		{
			completed();
			return;
		}

		var paramList = ParamTab.prepareParamRequest(postParams);
		saveNextParam(paramList);
	}

	function saveNextParam(paramList)
	{
		if (paramList.length > 0)
		{
			RPC.call('editqueue', ['GroupSetParameter', paramList[0], [curGroup.NZBID]], function()
			{
				notification = '#Notif_Downloads_Saved';
				paramList.shift();
				saveNextParam(paramList);
			})
		}
		else
		{
			saveFiles();
		}
	}

	/*** TAB: DUPLICATE SETTINGS **************************************************/

	function saveDupeKey()
	{
		var value = $('#DownloadsEdit_DupeKey').val();
		value !== curGroup.DupeKey ?
			RPC.call('editqueue', ['GroupSetDupeKey', value, [curGroup.NZBID]], function()
			{
				notification = '#Notif_Downloads_Saved';
				saveDupeScore();
			})
			:saveDupeScore();
	}

	function saveDupeScore()
	{
		var value = $('#DownloadsEdit_DupeScore').val();
		value != curGroup.DupeScore ?
			RPC.call('editqueue', ['GroupSetDupeScore', value, [curGroup.NZBID]], function()
			{
				notification = '#Notif_Downloads_Saved';
				saveDupeMode();
			})
			:saveDupeMode();
	}

	function saveDupeMode()
	{
		var value = $('#DownloadsEdit_DupeMode').val();
		value !== curGroup.DupeMode ?
			RPC.call('editqueue', ['GroupSetDupeMode', value, [curGroup.NZBID]], function()
			{
				notification = '#Notif_Downloads_Saved';
				saveParam();
			})
			:saveParam();
	}

	/*** TAB: FILES *************************************************************************/

	function fillFiles()
	{
		$('.loading-block', $DownloadsEditDialog).show();
		var requested = curGroup;
		RPC.call('listfiles', [0, 0, curGroup.NZBID], function(fileArr)
			{
				// a reply for a copy the panel has since moved off or closed:
				// its file ids must never reach this copy's file actions
				if (curGroup === requested && $DownloadsEditDialog.hasClass('in'))
				{
					filesLoaded(fileArr);
				}
			});
	}

	function filesLoaded(fileArr)
	{
		$('.loading-block', $DownloadsEditDialog).hide();

		files = fileArr;

		var data = [];

		for (var i=0; i < files.length; i++)
		{
			var file = files[i];

			if (!file.status)
			{
				file.status = file.Paused ? (file.ActiveDownloads > 0 ? 'pausing' : 'paused') : (file.ActiveDownloads > 0 ? 'downloading' : 'queued');
			}

			var FileSizeMB = (file.FileSizeHi * 4096) + (file.FileSizeLo / 1024 / 1024);
			var RemainingSizeMB = (file.RemainingSizeHi * 4096) + (file.RemainingSizeLo / 1024 / 1024);
			var age = Util.formatAge(file.PostTime + UISettings.timeZoneCorrection*60*60);
			var size = Util.formatSizeMB(FileSizeMB, file.FileSizeLo);
			if (FileSizeMB !== RemainingSizeMB || file.FileSizeLo !== file.RemainingSizeLo)
			{
				size = '(' + Util.round0((file.FileSizeHi > 0 ?
					RemainingSizeMB / FileSizeMB :
					file.RemainingSizeLo / file.FileSizeLo) * 100) + '%) ' + size;
			}

			var status;
			switch (file.status)
			{
				case 'downloading': status = '<span class="label label-status label-success">' + I18n.translate('status_downloading_cap') + '</span>'; break;
				case 'pausing': status = '<span class="label label-status label-success">' + I18n.translate('status_pausing_cap') + '</span>'; break;
				case 'paused': status = '<span class="label label-status label-warning">' + I18n.translate('status_paused_cap') + '</span>'; break;
				case 'queued': status = '<span class="label label-status">' + I18n.translate('status_queued_cap') + '</span>'; break;
				case 'deleted': status = '<span class="label label-status label-important">' + I18n.translate('status_deleted_cap') + '</span>'; break;
				default: status = '<span class="label label-status label-important">' + I18n.translate('label_internal_error') + '(' + file.status + ')</span>';
			}

			var name = Util.textToHtml(file.Filename);
			var fields;

			if (!UISettings.miniTheme)
			{
				var info = name;
				fields = ['<div class="check img-check"></div>', status, info, age, size];
			}
			else
			{
				var info = '<div class="check img-check"></div><span class="row-title">' + name + '</span>' +
					' ' + (file.status === 'queued' ? '' : status);
				fields = [info];
			}

			var item =
			{
				id: file.ID,
				file: file,
				fields: fields,
				data: { status: file.status, name: file.Filename, age: age, size: size, _search: true }
			};

			data.push(item);
		}

		$DownloadsFileTable.fasttable('update', data);
	}

	function fileTableRenderCellCallback(cell, index, item)
	{
		if (index > 2)
		{
			cell.className = 'text-right';
		}
	}

	this.editActionClick = function(action)
	{
		if (files.length == 0)
		{
			return;
		}

		var checkedRows = $DownloadsFileTable.fasttable('checkedRows');
		var checkedCount = $DownloadsFileTable.fasttable('checkedCount');
		if (checkedCount === 0)
		{
			PopupNotification.show('#Notif_Edit_Select');
			return;
		}

		for (var i = 0; i < files.length; i++)
		{
			var file = files[i];
			file.moved = false;
		}

		var editIDList = [];
		var splitError = false;

		for (var i = 0; i < files.length; i++)
		{
			var n = i;
			if (action === 'down' || action === 'top')
			{
				// iterate backwards in the file list
				n = files.length-1-i;
			}
			var file = files[n];

			if (checkedRows[file.ID])
			{
				editIDList.push(file.ID);

				switch (action)
				{
					case 'pause':
						file.status = 'paused';
						file.editAction = action;
						break;
					case 'resume':
						file.status = 'queued';
						file.editAction = action;
						break;
					case 'delete':
						file.status = 'deleted';
						file.editAction = action;
						break;
					case 'top':
						if (!file.moved)
						{
							files.splice(n, 1);
							files.unshift(file);
							file.moved = true;
							file.editMoved = true;
							i--;
						}
						break;
					case 'up':
						if (!file.moved && i > 0)
						{
							files.splice(i, 1);
							files.splice(i-1, 0, file);
							file.moved = true;
							file.editMoved = true;
						}
						break;
					case 'down':
						if (!file.moved && i > 0)
						{
							files.splice(n, 1);
							files.splice(n+1, 0, file);
							file.moved = true;
							file.editMoved = true;
						}
						break;
					case 'bottom':
						if (!file.moved)
						{
							files.splice(i, 1);
							files.push(file);
							file.moved = true;
							file.editMoved = true;
							i--;
						}
						break;
					case 'split':
						if (file.ActiveDownloads > 0 || file.Progress > 0)
						{
							splitError = true;
						}
						break;
				}
			}
		}

		if (action === 'split')
		{
			if (splitError)
			{
				PopupNotification.show('#Notif_Downloads_SplitNotPossible');
			}
			else
			{
				DownloadsSplitDialog.showModal(curGroup, editIDList);
			}
		}

		filesLoaded(files);
	}

	function saveFilesActions(actions, commands)
	{
		if (actions.length === 0 || !files || files.length === 0)
		{
			saveFileOrder();
			return;
		}

		var action = actions.shift();
		var command = commands.shift();

		var IDs = [];
		for (var i = 0; i < files.length; i++)
		{
			var file = files[i];
			if (file.editAction === action)
			{
				IDs.push(file.ID);
			}
		}

		if (IDs.length > 0)
		{
			RPC.call('editqueue', [command, '', IDs], function()
			{
				notification = '#Notif_Downloads_Saved';
				saveFilesActions(actions, commands);
			})
		}
		else
		{
			saveFilesActions(actions, commands);
		}
	}

	function saveFiles()
	{
		saveFilesActions(['pause', 'resume', 'delete'], ['FilePause', 'FileResume', 'FileDelete']);
	}

	function saveFileOrder()
	{
		if (!files || files.length === 0)
		{
			completed();
			return;
		}

		var IDs = [];
		var hasMovedFiles = false;
		for (var i = 0; i < files.length; i++)
		{
			var file = files[i];
			IDs.push(file.ID);
			hasMovedFiles |= file.editMoved;
		}

		if (hasMovedFiles)
		{
			RPC.call('editqueue', ['FileReorder', '', IDs], function()
			{
				notification = '#Notif_Downloads_Saved';
				completed();
			})
		}
		else
		{
			completed();
		}
	}

	/*** TAB: PER-SERVER STATUSTICS *****************************************************************/

	function scheduleRefresh()
	{
		refreshTimer = setTimeout(updateServStats, UISettings.refreshInterval * 1000);
	}

	function updateServStats()
	{
		RPC.call('listgroups', [], groups_loaded);
	}

	function groups_loaded(groups)
	{
		for (var i=0, il=groups.length; i < il; i++)
		{
			var group = groups[i];
			if (group.NZBID === curGroup.NZBID)
			{
				curGroup.ServerStats = group.ServerStats;
				EditUI.fillServStats($ServStatsTable, group);
				scheduleRefresh();
				break;
			}
		}
	}
}(jQuery));


/*** COMMON FUNCTIONS FOR EDIT DIALOGS ************************************************************/

var EditUI = (new function($)
{
	'use strict'

	/*** TAB: SERVER STATISTICS **************************************************/

	this.fillServStats = function(table, editItem)
	{
		var data = [];

		if (!Status.status || !Status.status.NewsServers)
		{
			table.fasttable('update', data);
			return;
		}

		for (var i=0; i < Status.status.NewsServers.length; i++)
		{
			var server = Status.status.NewsServers[i];
			var name = Options.option('Server' + server.ID + '.Name');
			if (name === null || name === '')
			{
				var host = Options.option('Server' + server.ID + '.Host');
				var port = Options.option('Server' + server.ID + '.Port');
				name = (host === null ? '' : host) + ':' + (port === null ? '119' : port);
			}

			var articles = '--';
			var artquota = '--';
			var success = '--';
			var failures = '--';
			for (var j=0; j < editItem.ServerStats.length; j++)
			{
				var stat = editItem.ServerStats[j];
				if (stat.ServerID === server.ID && stat.SuccessArticles + stat.FailedArticles > 0)
				{
					articles = stat.SuccessArticles + stat.FailedArticles;
					artquota = Util.round0(articles * 100.0 / (editItem.SuccessArticles + editItem.FailedArticles)) + '%';
					success = Util.round0(stat.SuccessArticles * 100.0 / articles) + '%';
					failures = Util.round0(stat.FailedArticles * 100.0 / articles) + '%';
					if (stat.FailedArticles > 0 && failures === '0%')
					{
						success = '99.9%';
						failures = '0.1%';
					}
					success = '<span title="' + stat.SuccessArticles + ' article' + (stat.SuccessArticles === 1 ? '' : 's') + '">' + success + '</span>';
					failures = '<span title="' + stat.FailedArticles + ' article' + (stat.FailedArticles === 1 ? '' : 's') + '">' + failures + '</span>';
					break;
				}
			}

			var fields = [server.ID + '. ' + Util.textToHtml(name), articles, artquota, success, failures];
			var item =
			{
				id: server.ID,
				fields: fields,
			};
			data.push(item);
		}
		table.fasttable('update', data);
	}

	this.servStatsTableRenderCellCallback = function (cell, index, item)
	{
		if (index > 0)
		{
			cell.className = 'text-right';
		}
	}

}(jQuery));


/*** PARAM TAB FOR EDIT DIALOGS ************************************************************/

var ParamTab = (new function($)
{
	'use strict'

	this.buildPostParamTab = function(configData, postParamConfig, parameters)
	{
		var postParams = $.extend(true, [], postParamConfig);
		Options.mergeValues(postParams, parameters);
		var content = Config.buildOptionsContent(postParams[0]);
		configData.empty();
		configData.append(content);
		configData.addClass('retain-margin');

		var lastClass = '';
		var lastDiv = null;
		for (var i=0; i < configData.children().length; i++)
		{
			var div = $(configData.children()[i]);
			var divClass = div.attr('class');
			if (divClass != lastClass && lastClass != '')
			{
				lastDiv.addClass('wants-divider');
			}
			lastDiv = div;
			lastClass = divClass;
		}
		if (window.I18n)
		{
			I18n.translatePage(configData);
			if (window.Config)
			{
				Config._translateDescriptions();
			}
		}
		return postParams;
	}

	this.createPostParamConfig = function()
	{
		var postParamConfig = Options.postParamConfig;
		defineBuiltinParams(postParamConfig);
		return postParamConfig;
	}

	this.updatePostParamTranslations = function()
	{
		$('.control-group._Unpack_').each(function()
		{
			var $option = $(this);
			var $label = $option.find('.option-name');
			var $about = $option.find('.help-block');
			var optId = $label.attr('data-optid');
			
			if (optId === '_Unpack_Password')
			{
				if ($about.length > 0)
				{
					$about.find('.help-option-title').text(I18n.translate('label_unpack_about'));
				}
			}
			else if (optId === '_Unpack_')
			{
				if ($about.length > 0)
				{
					$about.find('.help-option-title').text(I18n.translate('label_unpack_rar_about'));
				}
			}
		});

		if (window.Config)
		{
			Config._translateDescriptions();
		}
	}

	function defineBuiltinParams(postParamConfig)
	{
	    if (postParamConfig.length == 0)
	    {
	        postParamConfig.push({category: 'P', postparam: true, options: []});
	    }

		if (!Options.findOption(postParamConfig[0].options, '*Unpack:'))
		{
			postParamConfig[0].options.unshift({
				name: '*Unpack:Password', 
				value: '', 
				defvalue: '', 
				select: [], 
				caption: 'Password', 
				sectionId: '_Unpack_', 
				about: I18n.translate('label_unpack_about'), 
				description: '',
				requirements: [],
			});
			postParamConfig[0].options.unshift({
				name: '*Unpack:', 
				value: '', 
				defvalue: 'yes', 
				select: ['yes', 'no'], 
				caption: 'Unpack', 
				sectionId: '_Unpack_', 
				about: I18n.translate('label_unpack_rar_about'), 
				description: '',
				requirements: [],
			});
		}
	}

	this.prepareParamRequest = function(postParams)
	{
		var request = [];
		for (var i=0; i < postParams.length; i++)
		{
			var section = postParams[i];
			for (var j=0; j < section.options.length; j++)
			{
				var option = section.options[j];
				if (!option.template && !section.hidden)
				{
					var oldValue = option.value;
					var newValue = Config.getOptionValue(option);
					if (oldValue != newValue && !((oldValue === null || oldValue === '') && newValue === option.defvalue))
					{
						var opt = option.name + '=' + newValue;
						request.push(opt);
					}
				}
			}
		}
		return request;
	}

	// The post-processing switches enabled for a category: just the built-in
	// *Unpack, following the category's (or the global) Unpack option.
	function buildCategoryScriptList(category)
	{
		for (var i=0; i < Options.categories.length; i++)
		{
			if (category === Options.categories[i])
			{
				return Options.option('Category' + (i + 1) + '.Unpack') === 'yes' ? ['*Unpack'] : [];
			}
		}

		// empty category or category not found
		return Options.option('Unpack') === 'yes' ? ['*Unpack'] : [];
	}

	this.reassignParams = function(postParams, oldCategory, newCategory)
	{
		var oldScriptList = buildCategoryScriptList(oldCategory);
		var newScriptList = buildCategoryScriptList(newCategory);

		for (var i=0; i < postParams.length; i++)
		{
			var section = postParams[i];
			for (var j=0; j < section.options.length; j++)
			{
				var option = section.options[j];
				if (!option.template && !section.hidden && option.name.substr(option.name.length - 1, 1) === ':')
				{
					var scriptName = option.name.substr(0, option.name.length-1);
					if (oldScriptList.indexOf(scriptName) > -1 && newScriptList.indexOf(scriptName) === -1)
					{
						Config.setOptionValue(option, 'no');
					}
					else if (oldScriptList.indexOf(scriptName) === -1 && newScriptList.indexOf(scriptName) > -1)
					{
						Config.setOptionValue(option, 'yes');
					}
				}
			}
		}
	}

}(jQuery));


/*** LOG TAB FOR EDIT DIALOGS ************************************************************/

var LogTab = (new function($)
{
	'use strict'

	var curLog;
	var curItem;
	var curName;

	this.init = function(name)
	{
		var recordsPerPage = UISettings.read('ItemLogRecordsPerPage', 10);
		$('#' + name + 'LogRecordsPerPage').val(recordsPerPage);

		var $LogTable = $('#' + name + 'Edit_LogTable');
		$LogTable.fasttable(
			{
				filterInput: '#' + name + 'Edit_LogTable_filter',
				pagerContainer: '#' + name + 'Edit_LogTable_pager',
				pageSize: recordsPerPage,
				maxPages: 3,
				renderCellCallback: logTableRenderCellCallback
			});
	}

	this.reset = function(name)
	{
		// both panels share this tab: closing one must not drop the other's log
		if (name === curName)
		{
			curItem = null;
			curLog = null;
		}
		$('#' + name + 'EditDialog .loading-block').hide();
		$('#' + name + 'Edit_LogEmpty').hide();
		var $LogTable = $('#' + name + 'Edit_LogTable');
		$LogTable.fasttable('update', []);
		$LogTable.fasttable('setCurPage', 1);
		$LogTable.fasttable('applyFilter', '');

		$('#' + name + 'Edit_LogTable_filter').val('');
	}

	this.fill = function(name, item)
	{
		curItem = item;
		curName = name;
		curLog = null;
		var requested = item;

		function logLoaded(log)
		{
			// a reply for a copy the panel has since moved off
			if (curItem !== requested)
			{
				return;
			}
			curLog = log;

			$('#' + name + 'EditDialog .loading-block').hide();
			var $LogTable = $('#' + name + 'Edit_LogTable');
			var data = [];

			for (var i=0; i < log.length; i++)
			{
				var message = log[i];

				var kind;
				switch (message.Kind)
				{
					case 'INFO': kind = '<span class="label label-status label-success">' + I18n.translate('label_info') + '</span>'; break;
					case 'DETAIL': kind = '<span class="label label-status label-info">' + I18n.translate('label_detail') + '</span>'; break;
					case 'WARNING': kind = '<span class="label label-status label-warning">' + I18n.translate('label_warning') + '</span>'; break;
					case 'ERROR': kind = '<span class="label label-status label-important">' + I18n.translate('label_error') + '</span>'; break;
					case 'DEBUG': kind = '<span class="label label-status">' + I18n.translate('label_debug') + '</span>'; break;
				}

				var text = Util.textToHtml(message.Text);
				var time = Util.formatDateTime(message.Time + UISettings.timeZoneCorrection*60*60);
				var fields;

				if (!UISettings.miniTheme)
				{
					fields = [kind, time, text];
				}
				else
				{
					var info = kind + ' <span class="label">' + time + '</span> ' + text;
					fields = [info];
				}

				var item =
				{
					id: message,
					fields: fields,
					data: { kind: message.Kind, time: time, text: message.Text, _search: true }
				};

				data.unshift(item);
			}

			$LogTable.fasttable('update', data);
			Util.show('#' + name + 'Edit_LogEmpty', data.length === 0);
		}

		var recordsPerPage = UISettings.read('ItemLogRecordsPerPage', 10);
		$('#' + name + 'LogRecordsPerPage').val(recordsPerPage);

		$('#' + name + 'EditDialog .loading-block').show();
		RPC.call('loadlog', [item.NZBID, 0, 10000], logLoaded);
	}

	function logTableRenderCellCallback(cell, index, item)
	{
	}

	this.recordsPerPageChange = function(name)
	{
		var val = $('#' + name + 'LogRecordsPerPage').val();
		UISettings.write('ItemLogRecordsPerPage', val);
		var $LogTable = $('#' + name + 'Edit_LogTable');
		$LogTable.fasttable('setPageSize', val);
	}

	this.export = function()
	{
		if (!curItem || !curLog)
		{
			return;
		}
		var filename = curItem.NZBName + '.log';
		var logstr = '';

		for (var i=0; i < curLog.length; i++)
		{
			var message = curLog[i];
			var time = Util.formatDateTime(message.Time + UISettings.timeZoneCorrection*60*60);
			logstr += time + '\t' + message.Kind + '\t' + message.Text + '\n';
		}

		if (!Util.saveToLocalFile(logstr, "text/plain;charset=utf-8", filename))
		{
			var queueDir = Options.option('QueueDir');
			var pathSeparator = queueDir.indexOf('\\') > -1 ? '\\' : '/';
			alert(I18n.translate('msg_no_filesystem_access', queueDir + pathSeparator + 'n' + curItem.NZBID + '.log'));
		}
	}
}(jQuery));


/*** DOWNLOAD MULTI EDIT DIALOG ************************************************************/

var DownloadsMultiDialog = (new function($)
{
	'use strict'

	// Controls
	var $DownloadsMultiDialog;

	// State
	var multiIDList;
	var notification = null;
	var oldPriority;
	var oldCategory;

	this.init = function()
	{
		$DownloadsMultiDialog = $('#DownloadsMultiDialog');

		$('#DownloadsMulti_Save').click(saveChanges);

		$DownloadsMultiDialog.on('hidden', function ()
		{
			Refresher.resume();
		});

		if (UISettings.setFocus)
		{
			$DownloadsMultiDialog.on('shown', function ()
			{
				if ($('#DownloadsMulti_Priority').is(":visible"))
				{
					$('#DownloadsMulti_Priority').focus();
				}
			});
		}
	}

	this.showModal = function(nzbIdList, allGroups)
	{
		var groups = [];
		multiIDList = [];

		for (var i=0; i<allGroups.length; i++)
		{
			var gr = allGroups[i];
			if (nzbIdList.indexOf(gr.NZBID) > -1)
			{
				groups.push(gr);
				multiIDList.push(gr.NZBID);
			}
		}
		if (groups.length == 0)
		{
			return;
		}

		Refresher.pause();

		var FileSizeMB = 0, FileSizeLo = 0;
		var RemainingSizeMB = 0, RemainingSizeLo = 0;
		var PausedSizeMB = 0, PausedSizeLo = 0;
		var FileCount = 0, RemainingFileCount = 0, RemainingParCount = 0;
		var paused = true;
		var Priority = groups[0].MaxPriority;
		var PriorityDiff = false;
		var Category = groups[0].Category;
		var CategoryDiff = false;

		for (var i=0; i<groups.length; i++)
		{
			var group = groups[i];
			FileSizeMB += group.FileSizeMB;
			FileSizeLo += group.FileSizeLo;
			RemainingSizeMB += group.RemainingSizeMB;
			RemainingSizeLo += group.RemainingSizeLo;
			PausedSizeMB += group.PausedSizeMB;
			PausedSizeLo += group.PausedSizeLo;
			FileCount += group.FileCount;
			RemainingFileCount += group.RemainingFileCount;
			RemainingParCount += group.RemainingParCount;
			paused = paused && group.Status === 'PAUSED';
			PriorityDiff = PriorityDiff || (Priority !== group.MaxPriority);
			CategoryDiff = CategoryDiff || (Category !== group.Category);
		}

		var size = Util.formatSizeMB(FileSizeMB, FileSizeLo);
		var remaining = Util.formatSizeMB(RemainingSizeMB-PausedSizeMB, RemainingSizeLo-PausedSizeLo);
		var unpausedSize = Util.formatSizeMB(PausedSizeMB, PausedSizeLo);
		var estimated = paused ? '' : (Status.status && Status.status.DownloadRate > 0 ? Util.formatTimeHMS((RemainingSizeMB-PausedSizeMB)*1024/(Status.status.DownloadRate/1024)) : '');

		var table = '';
		table += '<tr><td>Total</td><td class="text-right">' + size + '</td></tr>';
		table += '<tr><td>Paused</td><td class="text-right">' + unpausedSize + '</td></tr>';
		table += '<tr><td>Unpaused</td><td class="text-right">' + remaining + '</td></tr>';
		table += '<tr><td>Estimated time</td><td class="text-right">' + estimated + '</td></tr>';
		table += '<tr><td>Files (total/remaining/pars)</td><td class="text-center">' + FileCount + ' / ' +
			RemainingFileCount + ' / ' + RemainingParCount + '</td></tr>';
		$('#DownloadsMulti_Statistics').html(table);

		$('#DownloadsMulti_Title').text(I18n.translate('msg_multiple_records_count', groups.length));

		// Priority
		var v = $('#DownloadsMulti_Priority');
		DownloadsUI.fillPriorityCombo(v);
		v.val(Priority);
		if (v.val() != Priority)
		{
			v.append('<option>' + Priority +'</option>');
			v.val(Priority);
		}
		if (PriorityDiff)
		{
			v.append('<option selected="selected">&lt;multiple values&gt;</option>');
		}
		oldPriority = v.val();
		$('#DownloadsMulti_Priority').removeAttr('disabled');

		// Category
		var v = $('#DownloadsMulti_Category');
		DownloadsUI.fillCategoryCombo(v);
		v.val(Category);
		if (v.val() != Category)
		{
			v.append($('<option></option>').text(Category));
			v.val(Category);
		}
		if (CategoryDiff)
		{
			v.append('<option selected="selected">&lt;multiple values&gt;</option>');
		}
		oldCategory = v.val();

		enableAllButtons();
		$('#DownloadsMulti_GeneralTabLink').tab('show');

		notification = null;

		$DownloadsMultiDialog.modal({backdrop: 'static'});
	}

	function enableAllButtons()
	{
		$('#DownloadsMulti .modal-footer .btn').removeAttr('disabled');
		$('#DownloadsMulti_Transmit').hide();
	}

	function disableAllButtons()
	{
		$('#DownloadsMulti .modal-footer .btn').attr('disabled', 'disabled');
		setTimeout(function()
		{
			$('#DownloadsMulti_Transmit').show();
		}, 500);
	}

	function saveChanges(e)
	{
		e.preventDefault();
		disableAllButtons();
		savePriority();
	}

	function savePriority()
	{
		var priority = $('#DownloadsMulti_Priority').val();
		(priority !== oldPriority && priority !== '<multiple values>') ?
			RPC.call('editqueue', ['GroupSetPriority', priority, multiIDList], function()
			{
				notification = '#Notif_Downloads_Saved';
				saveCategory();
			})
			: saveCategory();
	}

	function saveCategory()
	{
		var category = $('#DownloadsMulti_Category').val();
		(category !== oldCategory && category !== '<multiple values>') ?
			RPC.call('editqueue', ['GroupApplyCategory', category, multiIDList], function()
			{
				notification = '#Notif_Downloads_Saved';
				completed();
			})
			: completed();
	}

	function completed()
	{
		$DownloadsMultiDialog.modal('hide');
		Refresher.update();
		if (notification)
		{
			PopupNotification.show(notification);
		}
	}
}(jQuery));


/*** DOWNLOAD MERGE DIALOG ************************************************************/

var DownloadsMergeDialog = (new function($)
{
	'use strict'

	// Controls
	var $DownloadsMergeDialog;

	// State
	var mergeEditIDList;

	this.init = function()
	{
		$DownloadsMergeDialog = $('#DownloadsMergeDialog');

		$('#DownloadsMerge_Merge').click(merge);

		$DownloadsMergeDialog.on('hidden', function ()
		{
			Refresher.resume();
		});

		if (UISettings.setFocus)
		{
			$DownloadsMergeDialog.on('shown', function ()
			{
				$('#DownloadsMerge_Merge').focus();
			});
		}
	}

	this.showModal = function(nzbIdList, allGroups)
	{
		Refresher.pause();

		mergeEditIDList = [];
		var $tbody = $('#DownloadsMergeTable tbody').empty();
		for (var i = 0; i < allGroups.length; i++)
		{
			var group = allGroups[i];
			if (nzbIdList.indexOf(group.NZBID) > -1)
			{
				mergeEditIDList.push(group.NZBID);
				var html = '<table><tr><td width="18px" valign="top"><i class="material-icon" style="vertical-align:top;margin-top:2px;">draft</i></td><td>' +
					Util.textToHtml(Util.formatNZBName(group.NZBName)) + '</td></tr></table>';
				$tbody.append(html);
			}
		}
		if (typeof I18n !== 'undefined' && I18n.translatePage)
		{
			I18n.translatePage($tbody);
		}

		$DownloadsMergeDialog.modal({backdrop: 'static'});
	}

	function merge()
	{
		RPC.call('editqueue', ['GroupMerge', '', mergeEditIDList], completed);
	}

	function completed()
	{
		$DownloadsMergeDialog.modal('hide');
		Refresher.update();
		PopupNotification.show('#Notif_Downloads_Merged');
	}
}(jQuery));


/*** DOWNLOAD SPLIT DIALOG ************************************************************/

var DownloadsSplitDialog = (new function($)
{
	'use strict'

	// Controls
	var $DownloadsSplitDialog;

	// State
	var splitEditIDList;

	this.init = function()
	{
		$DownloadsSplitDialog = $('#DownloadsSplitDialog');

		$('#DownloadsSplit_Split').click(split);

		$DownloadsSplitDialog.on('hidden', function ()
		{
			Refresher.resume();
		});

		if (UISettings.setFocus)
		{
			$DownloadsSplitDialog.on('shown', function ()
			{
				$('#DownloadsSplit_Merge').focus();
			});
		}
	}

	this.showModal = function(group, editIDList)
	{
		Refresher.pause();
		splitEditIDList = editIDList;
		var groupName = group.NZBName + ' (' + editIDList[0] + (editIDList.length > 1 ? '-' + editIDList[editIDList.length-1] : '') + ')';
		$('#DownloadsSplit_NZBName').attr('value', groupName);
		$DownloadsSplitDialog.modal({backdrop: 'static'});
	}

	function split()
	{
		var groupName = $('#DownloadsSplit_NZBName').val();
		RPC.call('editqueue', ['FileSplit', groupName, splitEditIDList], completed);
	}

	function completed(result)
	{
		$('#DownloadsEditDialog').modal('hide');
		$DownloadsSplitDialog.modal('hide');
		Refresher.update();
		PopupNotification.show(result ? '#Notif_Downloads_Splitted' : '#Notif_Downloads_SplitError');
	}
}(jQuery));


/*** EDIT HISTORY DIALOG *************************************************************************/

var HistoryEditDialog = (new function($)
{
	'use strict'

	// Controls
	var $HistoryEditDialog;
	var $HistoryEdit_ParamData;
	var $ServStatsTable;

	// State
	var curHist;
	var notification = null;
	var postParams = [];
	var lastPage;
	var lastFullscreen;
	var saveCompleted;
	var logFilled;
	var showing;

	this.init = function()
	{
		$HistoryEditDialog = $('#HistoryEditDialog');
		$HistoryEdit_ParamData = $('#HistoryEdit_ParamData');

		$('#HistoryEdit_Save').click(saveChanges);
		$('#HistoryEdit_Actions').click(itemActions);
		$('#HistoryEdit_Param, #HistoryEdit_Dupe, #HistoryEdit_Log').click(tabClick);
		$('#HistoryEdit_Back').click(backClick);

		LogTab.init('History');

		$ServStatsTable = $('#HistoryEdit_ServStatsTable');
		$ServStatsTable.fasttable(
			{
				filterInput: '#HistoryEdit_ServStatsTable_filter',
				pagerContainer: '#HistoryEdit_ServStatsTable_pager',
				pageSize: 100,
				maxPages: 3,
				renderCellCallback: EditUI.servStatsTableRenderCellCallback
			});

		$HistoryEditDialog.on('hidden', function ()
		{
			$HistoryEdit_ParamData.empty();
			LogTab.reset('History');
			// resume updates
			Refresher.resume();
		});

		$HistoryEditDialog.on('shown', function ()
		{
			if ($HistoryEdit_ParamData && $HistoryEdit_ParamData.children().length > 0)
			{
				ParamTab.updatePostParamTranslations();
			}
		});

		TabDialog.extend($HistoryEditDialog);
		DetailsPanel.bindFleet($('#HistoryEdit_Fleet'));

		I18n.subscribe(function()
		{
			if ($HistoryEdit_ParamData && $HistoryEdit_ParamData.children().length > 0)
			{
				ParamTab.updatePostParamTranslations();
			}
		});
	}

	this.showModal = function(hist, area)
	{
		// the panel is already open on another copy of the fleet: switch over
		var switching = $HistoryEditDialog.hasClass('in');
		if (!switching)
		{
			Refresher.pause();
		}

		curHist = hist;

		var status = '';
		if (hist.Kind === 'NZB')
		{
			if (hist.DeleteStatus === '' || hist.DeleteStatus === 'HEALTH')
			{
				status = '<span class="label label-status ' +
					(hist.Health === 1000 ? 'label-success' : hist.Health >= hist.CriticalHealth ? 'label-warning' : 'label-important') +
					'">' + I18n.translate('status_health', Math.floor(hist.Health / 10)) + '</span>';
			}

			if (hist.MarkStatus !== 'NONE')
			{
				status += ' ' + buildStatus(hist.MarkStatus, I18n.translate('status_mark'));
			}

			else if (hist.DeleteStatus === 'NONE')
			{
				var exParStatus = hist.ExParStatus === 'RECIPIENT' ? ' ' + '<span title="' + I18n.translate('tooltip_expar_recipient', hist.ExtraParBlocks, (hist.ExtraParBlocks > 1 ? 's' : '')) + '">' + buildStatus(hist.ExParStatus, I18n.translate('status_expar')) + '</span>' :
					hist.ExParStatus === 'DONOR' ? ' ' + '<span title="' + I18n.translate('tooltip_expar_donor', -hist.ExtraParBlocks, (-hist.ExtraParBlocks > 1 ? 's' : '')) + '">' + buildStatus(hist.ExParStatus, I18n.translate('status_expar')) + '</span>' : '';
				status += ' ' + buildStatus(hist.ParStatus, I18n.translate('status_par')) + exParStatus +
					' ' + (Options.option('Unpack') == 'yes' || hist.UnpackStatus != 'NONE' ? buildStatus(hist.UnpackStatus, I18n.translate('status_unpack')) : '')  +
					' ' + (hist.MoveStatus === "FAILURE" ? buildStatus(hist.MoveStatus, I18n.translate('status_move')) : '');
			}
			else
			{
				status += ' ' + buildStatus('DELETED-' + hist.DeleteStatus, I18n.translate('status_delete'));
			}

			for (var i=0; i<hist.ScriptStatuses.length; i++)
			{
				var scriptStatus = hist.ScriptStatuses[i];
				status += ' ' + buildStatus(scriptStatus.Status, Util.textToHtml(Options.shortScriptName(scriptStatus.Name)) + ': ') + ' ';
			}
		}
		else if (hist.Kind === 'URL')
		{
			if (hist.DeleteStatus !== 'NONE')
			{
				status = buildStatus('DELETED-' + hist.DeleteStatus, I18n.translate('status_delete'));
			}
			else if (hist.UrlStatus == 'SCAN_SKIPPED')
			{
				status = buildStatus('SUCCESS', I18n.translate('status_fetch')) + ' ' +
					buildStatus('SCAN_SKIPPED', I18n.translate('status_scan'));
			}
			else if (hist.UrlStatus == 'SCAN_FAILURE')
			{
				status = buildStatus('SUCCESS', I18n.translate('status_fetch')) + ' ' +
					buildStatus('FAILURE', I18n.translate('status_scan'));
			}
			else
			{
				status = buildStatus(hist.UrlStatus, I18n.translate('status_fetch'));
			}
		}
		else if (hist.Kind === 'DUP')
		{
			status = buildStatus(hist.DupStatus, '');
		}
		$('#HistoryEdit_Status').html(status);

		$('#HistoryEdit_Title').text(Util.formatNZBName(hist.Name));
		if (hist.Kind !== 'NZB')
		{
			$('#HistoryEdit_Title').html($('#HistoryEdit_Title').html() + '&nbsp;' + '<span class="label label-info">' +
				Util.textToHtml(hist.Kind === 'DUP' ? I18n.translate('label_hidden') : hist.Kind) + '</span>');
		}

		$('#HistoryEdit_NZBName').val(hist.Name);

		if (hist.Kind !== 'DUP')
		{
			// Category
			var v = $('#HistoryEdit_Category');
			DownloadsUI.fillCategoryCombo(v);
			v.val(hist.Category);
			if (v.val() != hist.Category)
			{
				v.append($('<option selected="selected"></option>').text(hist.Category));
			}
		}

		if (hist.Kind === 'NZB')
		{
			$('#HistoryEdit_Path').val(hist.FinalDir !== '' ? hist.FinalDir : hist.DestDir);
			$('#HistoryEdit_Summary').html(DetailsPanel.summary(hist, 'HistoryEdit'));

			$('#HistoryEdit_ServStats').click(tabClick);
			EditUI.fillServStats($ServStatsTable, hist);
			$ServStatsTable.fasttable('setCurPage', 1);

			$('#HistoryEdit_TimeStats').click(tabClick);
			fillTimeStats();
		}

		$('#HistoryEdit_DupeKey').val(hist.DupeKey);
		$('#HistoryEdit_DupeScore').val(hist.DupeScore);
		$('#HistoryEdit_DupeMode').val(hist.DupeMode);
		$('#HistoryEdit_DupeBackup').prop('checked', hist.DeleteStatus === 'DUPE');
		$('#HistoryEdit_DupeBackup').prop('disabled', !(hist.DeleteStatus === 'DUPE' || hist.DeleteStatus === 'MANUAL'));
		Util.show($('#HistoryEdit_DupeBackup').closest('.control-group'), hist.Kind === 'NZB');
		$('#HistoryEdit_DupeMode').closest('.control-group').toggleClass('last-group', hist.Kind !== 'NZB');

		$('#HistoryEdit_Fleet').html(hist.Kind === 'NZB' ? DetailsPanel.fleet(hist, 'history') : '');
		Util.show('#HistoryEdit_PathGroup, #HistoryEdit_Summary, #HistoryEdit_Fleet', hist.Kind === 'NZB');
		Util.show('#HistoryEdit_CategoryGroup', hist.Kind !== 'DUP');
		Util.show('#HistoryEdit_DupGroup', hist.Kind === 'DUP');
		var dupeCheck = Options.option('DupeCheck') === 'yes';
		Util.show('#HistoryEdit_Dupe', dupeCheck);
		$('#HistoryEdit_CategoryGroup').toggleClass('control-group-last', hist.Kind === 'URL');

		Util.show('#HistoryEdit_URLGroup', hist.Kind === 'URL');
		$('#HistoryEdit_URL').attr('value', hist.URL);

		var postParamConfig = ParamTab.createPostParamConfig();
		var postParam = hist.Kind === 'NZB' && postParamConfig[0].options.length > 0;
		Util.show('#HistoryEdit_Param', postParam);

		if (postParam)
		{
			postParams = ParamTab.buildPostParamTab($HistoryEdit_ParamData, postParamConfig, curHist.Parameters);
			ParamTab.updatePostParamTranslations();
		}

		// a finished download keeps its log in history, whatever its outcome
		var postLog = hist.Kind === 'NZB' || hist.MessageCount > 0;
		Util.show('#HistoryEdit_Log', postLog);

		enableAllButtons();

		$('#HistoryEdit_GeneralTab').show();
		$('#HistoryEdit_ParamTab').hide();
		$('#HistoryEdit_ServStatsTab').hide();
		$('#HistoryEdit_TimeStatsTab').hide();
		$('#HistoryEdit_DupeTab').hide();
		$('#HistoryEdit_LogTab').hide();
		$('#HistoryEdit_Back').hide();
		$('#HistoryEdit_BackSpace').show();
		$HistoryEditDialog.restoreTab();

		LogTab.reset('History');

		logFilled = false;
		notification = null;

		if (area === 'backup')
		{
			showing = true;
			$('#HistoryEdit_ServStats').trigger('click');
		}
		showing = false;

		if (!switching)
		{
			$HistoryEditDialog.modal({backdrop: 'static'});
			DetailsPanel.closeOnOutsideClick($HistoryEditDialog);
		}
	}

	function buildStatus(status, prefix)
	{
		switch (status)
		{
			case 'SUCCESS':
				return '<span class="label label-status label-success">' + prefix + I18n.translate('status_success') + '</span>';
			case 'GOOD':
				return '<span class="label label-status label-success">' + prefix + I18n.translate('status_good') + '</span>';
			case 'RECIPIENT':
				return '<span class="label label-status label-success">' + prefix + I18n.translate('status_recipient') + '</span>';
			case 'DONOR':
				return '<span class="label label-status label-success">' + prefix + I18n.translate('status_donor') + '</span>';
			case 'FAILURE':
				return '<span class="label label-status label-important">' + prefix + I18n.translate('status_failure') + '</span>';
			case 'BAD':
				return '<span class="label label-status label-important">' + prefix + I18n.translate('status_bad') + '</span>';
			case 'REPAIR_POSSIBLE':
				return '<span class="label label-status label-warning">' + prefix + I18n.translate('status_repairable') + '</span>';
			case 'MANUAL': // PAR-MANUAL
				return '<span class="label label-status label-warning">' + prefix + I18n.translate('status_manual') + '</span>';
			case 'SPACE':
				return '<span class="label label-status label-warning">' + prefix + I18n.translate('status_space') + '</span>';
			case 'PASSWORD':
				return '<span class="label label-status label-warning">' + prefix + I18n.translate('status_password') + '</span>';
			case 'DELETED-DUPE':
				return '<span class="label label-status">' + prefix + I18n.translate('status_dupe') + '</span>';
			case 'DELETED-MANUAL':
				return '<span class="label label-status">' + prefix + I18n.translate('status_manual') + '</span>';
			case 'DELETED-COPY':
				return '<span class="label label-status">' + prefix + I18n.translate('status_copy') + '</span>';
			case 'DELETED-GOOD':
				return '<span class="label label-status">' + prefix + I18n.translate('status_good') + '</span>';
			case 'DELETED-SUCCESS':
				return '<span class="label label-status">' + prefix + I18n.translate('status_success') + '</span>';
			case 'DELETED-HEALTH':
				return '<span class="label label-status label-important">' + prefix + I18n.translate('status_health_val') + '</span>';
			case 'DELETED-BAD':
				return '<span class="label label-status label-important">' + prefix + I18n.translate('status_bad') + '</span>';
			case 'DELETED-SCAN':
				return '<span class="label label-status label-important">' + prefix + I18n.translate('status_scan_val') + '</span>';
			case 'SCAN_SKIPPED':
				return '<span class="label label-status label-warning"">' + prefix + I18n.translate('status_skipped') + '</span>';
			case 'NONE':
				return '<span class="label label-status">' + prefix + I18n.translate('status_none') + '</span>';
			default:
				return '<span class="label label-status">' + prefix + status + '</span>';
		}
	}

	function fillTimeStats()
	{
		var hist = curHist;
		var downloaded = Util.formatSizeMB(hist.DownloadedSizeMB, hist.DownloadedSizeLo);
		var speed = hist.DownloadTimeSec > 0 ? Util.formatSpeed((hist.DownloadedSizeMB > 1024 ? hist.DownloadedSizeMB * 1024.0 * 1024.0 : hist.DownloadedSizeLo) / hist.DownloadTimeSec) : '--';
		var table = '';
		table += '<tr><td>' + I18n.translate('stat_downloaded_size') + '</td><td class="text-center">' + downloaded + '</td></tr>';
		table += '<tr><td>' + I18n.translate('stat_download_speed_dialog') + '</td><td class="text-center">' + speed + '</td></tr>';
		table += '<tr><td>' + I18n.translate('stat_total_time') + '</td><td class="text-center">' + Util.formatTimeHMS(hist.DownloadTimeSec + hist.PostTotalTimeSec) + '</td></tr>';
		table += '<tr><td>' + I18n.translate('stat_download_time_dialog') + '</td><td class="text-center">' + Util.formatTimeHMS(hist.DownloadTimeSec) + '</td></tr>';
		table += '<tr><td>' + I18n.translate('stat_verification_time') + ' </td><td class="text-center">' + Util.formatTimeHMS(hist.ParTimeSec - hist.RepairTimeSec) + '</td></tr>';
		table += '<tr><td>' + I18n.translate('stat_repair_time') + '</td><td class="text-center">' + Util.formatTimeHMS(hist.RepairTimeSec) + '</td></tr>';
		table += '<tr><td>' + I18n.translate('stat_unpack_time') + '</td><td class="text-center">' + Util.formatTimeHMS(hist.UnpackTimeSec) + '</td></tr>';
		table += hist.ExtraParBlocks > 0 ? '<tr><td>' + I18n.translate('stat_extra_par_blocks') + '</td><td class="text-center">' + hist.ExtraParBlocks + '</td></tr>' :
			hist.ExtraParBlocks < 0 ? '<tr><td>' + I18n.translate('stat_donated_par_blocks') + '</td><td class="text-center">' + - hist.ExtraParBlocks + '</td></tr>' : '';

		$('#HistoryEdit_TimeStatsTable tbody').html(table);
	}

	function tabClick(e)
	{
		e.preventDefault();

		$('#HistoryEdit_Back').fadeIn(showing ? 0 : 500);
		$('#HistoryEdit_BackSpace').hide();
		var tab = '#' + $(this).attr('data-tab');
		lastPage = $(tab);
		lastFullscreen = ($(this).attr('data-fullscreen') === 'true') && !UISettings.miniTheme;

		$HistoryEditDialog.switchTab($('#HistoryEdit_GeneralTab'), lastPage,
			e.shiftKey || !UISettings.slideAnimation || showing ? 0 : 500,
			{fullscreen: lastFullscreen, mini: UISettings.miniTheme});

		if (tab === '#HistoryEdit_LogTab' && !logFilled)
		{
			LogTab.fill('History', curHist);
			logFilled = true;
		}
	}

	function backClick(e)
	{
		e.preventDefault();
		$('#HistoryEdit_Back').fadeOut(500, function()
		{
			$('#HistoryEdit_BackSpace').show();
		});

		$HistoryEditDialog.switchTab(lastPage, $('#HistoryEdit_GeneralTab'),
			e.shiftKey || !UISettings.slideAnimation ? 0 : 500,
			{fullscreen: lastFullscreen, mini: UISettings.miniTheme, back: true});
	}

	function disableAllButtons()
	{
		$('#HistoryEditDialog .modal-footer .btn').attr('disabled', 'disabled');
		$('#HistoryEditDialog').addClass('is-saving');
		setTimeout(function()
		{
			$('#HistoryEdit_Transmit').show();
		}, 500);
	}

	function enableAllButtons()
	{
		$('#HistoryEditDialog .modal-footer .btn').removeAttr('disabled');
		$('#HistoryEditDialog').removeClass('is-saving');
		$('#HistoryEdit_Transmit').hide();
	}

	function completed()
	{
		$HistoryEditDialog.modal('hide');
		Refresher.update();
		if (notification)
		{
			PopupNotification.show(notification);
			notification = null;
		}
	}

	function saveChanges(e)
	{
		e.preventDefault();
		disableAllButtons();
		notification = null;
		saveCompleted = completed;
		saveName();
	}

	function saveName()
	{
		var name = $('#HistoryEdit_NZBName').val();
		name !== curHist.Name && !curHist.postprocess ?
			RPC.call('editqueue', ['HistorySetName', name, [curHist.ID]], function()
			{
				notification = '#Notif_History_Saved';
				saveCategory();
			})
			:saveCategory();
	}

	function saveCategory()
	{
		var category = $('#HistoryEdit_Category').val();
		category !== curHist.Category && curHist.Kind !== 'DUP' ?
			RPC.call('editqueue', ['HistorySetCategory', category, [curHist.ID]], function()
			{
				notification = '#Notif_History_Saved';
				saveDupeKey();
			})
			: saveDupeKey();
	}

	function itemActions(e)
	{
		e.preventDefault();
		e.stopPropagation();
		var elem = $('#HistoryEdit_Actions').parent();

		HistoryActionsMenu.showPopupMenu(curHist, 'top',
			{ left: elem.offset().left, top: elem.offset().top - 1,
				width: elem.width(), height: elem.height() + 2 },
			function(_notification, actionCallback)
			{
				disableAllButtons();
				notification = _notification;
				saveCompleted = actionCallback;
				saveName();
				return true; // async
			},
			completed);
	}

	/*** TAB: POST-PROCESSING PARAMETERS **************************************************/

	function saveParam()
	{
		if (curHist.Kind !== 'NZB')
		{
			saveCompleted();
			return;
		}

		var paramList = ParamTab.prepareParamRequest(postParams);
		saveNextParam(paramList);
	}

	function saveNextParam(paramList)
	{
		if (paramList.length > 0)
		{
			RPC.call('editqueue', ['HistorySetParameter', paramList[0], [curHist.ID]], function()
			{
				notification = '#Notif_History_Saved';
				paramList.shift();
				saveNextParam(paramList);
			})
		}
		else
		{
			saveCompleted();
		}
	}

	/*** TAB: DUPLICATE SETTINGS **************************************************/

	function saveDupeKey()
	{
		var value = $('#HistoryEdit_DupeKey').val();
		value !== curHist.DupeKey ?
			RPC.call('editqueue', ['HistorySetDupeKey', value, [curHist.ID]], function()
			{
				notification = '#Notif_History_Saved';
				saveDupeScore();
			})
			:saveDupeScore();
	}

	function saveDupeScore()
	{
		var value = $('#HistoryEdit_DupeScore').val();
		value != curHist.DupeScore ?
			RPC.call('editqueue', ['HistorySetDupeScore', value, [curHist.ID]], function()
			{
				notification = '#Notif_History_Saved';
				saveDupeMode();
			})
			:saveDupeMode();
	}

	function saveDupeMode()
	{
		var value = $('#HistoryEdit_DupeMode').val();
		value !== curHist.DupeMode ?
			RPC.call('editqueue', ['HistorySetDupeMode', value, [curHist.ID]], function()
			{
				notification = '#Notif_History_Saved';
				saveDupeBackup();
			})
			:saveDupeBackup();
	}

	function saveDupeBackup()
	{
		var canChange = curHist.DeleteStatus === 'DUPE' || curHist.DeleteStatus === 'MANUAL';
		var oldValue = curHist.DeleteStatus === 'DUPE';
		var value = $('#HistoryEdit_DupeBackup').is(':checked');
		canChange && value !== oldValue ?
			RPC.call('editqueue', ['HistorySetDupeBackup', value ? "YES" : "NO", [curHist.ID]], function()
			{
				notification = '#Notif_History_Saved';
				saveParam();
			})
			:saveParam();
	}
}(jQuery));
