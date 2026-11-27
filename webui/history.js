/*
 * This file is part of RustyGet.
 *
 * Copyright (C) 2012-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 * Copyright (C) 2024-2026 Denis <denis@nzbget.com>
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
 * In this module:
 *   1) History tab;
 *   2) Functions for html generation for history, also used from other modules (edit dialog);
 *   3) Popup menus in history list.
 */

/*** HISTORY TAB AND EDIT HISTORY DIALOG **********************************************/

var History = (new function($)
{
	'use strict';

	// Controls
	var $HistoryTable;
	var $HistoryTabBadge;
	var $HistoryTabBadgeEmpty;
	var $HistoryRecordsPerPage;
	var $CategoryMenu;

	// State
	var history = [];
	var firstLoad = true;
	var notification = null;
	var updateTabInfo;
	var curFilter = 'ALL';
	var activeTab = false;
	var showDup = false;
	var cached = false;
	var subscribers = [];

	this.subscribe = function(subscriber)
	{
		subscribers.push(subscriber);
	}

	function notifyAll(data)
	{
		subscribers.forEach(function(subject) { subject.update(data); });
	}

	this.init = function(options)
	{
		updateTabInfo = options.updateTabInfo;

		$HistoryTable = $('#HistoryTable');
		$HistoryTabBadge = $('#HistoryTabBadge');
		$HistoryTabBadgeEmpty = $('#HistoryTabBadgeEmpty');
		$HistoryRecordsPerPage = $('#HistoryRecordsPerPage');
		$CategoryMenu = $('#HistoryCategoryMenu');

		var recordsPerPage = UISettings.read('HistoryRecordsPerPage', 10);
		$HistoryRecordsPerPage.val(recordsPerPage);
		$('#HistoryTable_filter').val('');

		$HistoryTable.fasttable(
			{
				filterInput: '#HistoryTable_filter',
				filterClearButton: '#HistoryTable_clearfilter',
				pagerContainer: '#HistoryTable_pager',
				infoContainer: '#HistoryTable_info',
				rowSelect: UISettings.rowSelect,
				pageSize: recordsPerPage,
				maxPages: UISettings.miniTheme ? 1 : 5,
				pageDots: !UISettings.miniTheme,
				shortcuts: true,
				fillFieldsCallback: fillFieldsCallback,
				filterCallback: filterCallback,
				renderCellCallback: renderCellCallback,
				updateInfoCallback: updateInfo
			});

		$HistoryTable.on('click', 'a', editClick);
		$HistoryTable.on('click', 'td:nth-child(2).dropdown-cell > div', statusClick);
		$HistoryTable.on('click', 'td:nth-child(5).dropdown-cell > div:not(.dropdown-disabled)', categoryClick);
		$CategoryMenu.on('click', 'a', categoryMenuClick);

		HistoryActionsMenu.init();

		I18n.subscribe(function onLangChange() { History.redraw(true); });
	}

	this.applyTheme = function()
	{
		this.redraw(true);
		$HistoryTable.fasttable('setPageSize', UISettings.read('HistoryRecordsPerPage', 10),
			UISettings.miniTheme ? 1 : 5, !UISettings.miniTheme);
	}

	this.show = function()
	{
		activeTab = true;
		this.redraw(true);
	}

	this.hide = function()
	{
		activeTab = false;
	}

	this.update = function()
	{
		RPC.call('history', [showDup], loaded, null, { prefer_cached: true });
	}

	function loaded(_history, _cached)
	{
		// once, on the first load ("!history" never held: it starts as [], so the
		// DUPE filter stayed shown without DupeCheck)
		if (firstLoad)
		{
			firstLoad = false;
			$('#HistoryTable_Category').css('width', DownloadsUI.calcCategoryColumnWidth());
			initFilterButtons();
		}

		cached = _cached;
		if (!Refresher.isPaused() && !cached)
		{
			history = _history;
			prepare();
			// the details panel lists a release's copies across queue and history
			History.items = history;
		}
		notifyAll(history);
		RPC.next();
	}

	function prepare()
	{
		for (var j=0, jl=history.length; j < jl; j++)
		{
			var hist = history[j];
			if (hist.Status === 'DELETED/MANUAL' || hist.Status === 'DELETED/GOOD' ||
				hist.Status === 'DELETED/SUCCESS' || hist.Status === 'DELETED/COPY')
			{
				hist.FilterKind = 'DELETED';
			}
			else if (hist.Status === 'DELETED/DUPE')
			{
				hist.FilterKind = 'DUPE';
			}
			else if (hist.Status.substring(0, 7) === 'SUCCESS')
			{
				hist.FilterKind = 'SUCCESS';
			}
			else
			{
				hist.FilterKind = 'FAILURE';
			}
		}
	}

	this.redraw = function(force)
	{
		if (!Status.status)
		{
			return;
		}

		if (cached && !force)
		{
			return;
		}

		if (!Refresher.isPaused())
		{
			redraw_table();
		}

		Util.show($HistoryTabBadge, history.length > 0);
		Util.show($HistoryTabBadgeEmpty, history.length === 0 && UISettings.miniTheme);
	}

	var SEARCH_FIELDS = ['name', 'status', 'priority', 'category', 'age', 'size', 'time'];

	function redraw_table()
	{
		var data = [];

		for (var i=0; i < history.length; i++)
		{
			var hist = history[i];

			var kind = hist.Kind;
			hist.status = HistoryUI.buildStatusText(hist);
			hist.name = hist.Name;
			hist.size = kind === 'URL' && hist.FileSizeLo == 0 && hist.FileSizeHi == 0 ? '' : Util.formatSizeMB(hist.FileSizeMB, hist.FileSizeLo);
			hist.sizemb = hist.FileSizeMB;
			hist.sizegb = hist.FileSizeMB / 1024;
			hist.time = Util.formatDateTime(hist.HistoryTime + UISettings.timeZoneCorrection*60*60);
			hist.category = kind !== 'DUP' ? hist.Category : '';
			hist.dupe = DownloadsUI.buildDupeText(hist.DupeKey, hist.DupeScore, hist.DupeMode);
			// field search only (not free text): "repair:live", "repair:twin" or "repaired>0"
			// (slices plus borrowed articles)
			var repairRecord = HistoryUI.repairRecord(hist);
			hist.repair = repairRecord ? repairRecord.method.replace(/\+/g, ' ') + ' ' + HistoryUI.repairText(repairRecord) : '';
			hist.repaired = repairRecord ? repairRecord.slices + repairRecord.borrowed : 0;
			var age_sec = hist.MinPostTime > 0 ? new Date().getTime() / 1000 - (hist.MinPostTime + UISettings.timeZoneCorrection*60*60) : 0;
			hist.age = hist.MinPostTime > 0 ? Util.formatAge(hist.MinPostTime + UISettings.timeZoneCorrection*60*60) : '';
			hist.agem = Util.round0(age_sec / 60);
			hist.ageh = Util.round0(age_sec / (60*60));
			hist.aged = Util.round0(age_sec / (60*60*24));

			hist._search = SEARCH_FIELDS;

			var item =
			{
				id: hist.ID,
				data: hist,
			};

			data.push(item);
		}

		$HistoryTable.fasttable('update', data);
	}

	function fillFieldsCallback(item)
	{
		var hist = item.data;

		var status = HistoryUI.buildStatus(hist);
		var repair = hist.Kind === 'NZB' ? HistoryUI.buildRepairBadge(hist) : '';

		var name = '<a href="#" data-nzbid="' + hist.ID + '">' + Util.textToHtml(Util.formatNZBName(hist.Name)) + '</a>';
		name += DownloadsUI.buildEncryptedLabel(hist.Kind === 'NZB' ? hist.Parameters : []);

		var dupe = DownloadsUI.buildDupe(hist.DupeKey, hist.DupeScore, hist.DupeMode);
		var category = hist.Kind !== 'DUP' ?
			(hist.Category !== '' ? Util.textToHtml(hist.Category) : '<span class="none-category">' + I18n.translate('label_none') + '</span>')
			: '';
		var backup = hist.Kind === 'NZB' ? DownloadsUI.buildBackupLabel(hist) : '';
		var dupeScore = hist.Kind === 'NZB' ? DownloadsUI.buildDupeScoreLabel(hist.DupeScore, DownloadsUI.shownHealth(hist), hist.Parameters) +
			DownloadsUI.buildTwinGroupLabel(hist.Parameters, hist.Status.indexOf('DELETED/') !== 0) +
			(hist.Status === 'DELETED/DUPE' ? DownloadsUI.buildDonorLabel(hist.Parameters) : '') : '';

		if (hist.Kind === 'URL')
		{
			name += ' <span class="label label-info text-uppercase" data-i18n="label_kind_url">' + I18n.translate('label_kind_url') + '</span>';
		}
		else if (hist.Kind === 'DUP')
		{
			name += ' <span class="label label-info text-uppercase" data-i18n="label_hidden">' + I18n.translate('label_hidden') + '</span>';
		}

		if (!UISettings.miniTheme)
		{
			status = '<div data-nzbid="' + hist.NZBID + '">' + status + '</div>';
			category = '<div data-nzbid="' + hist.NZBID + '"' +  (hist.Kind === 'DUP' ? ' class="dropdown-disabled"' : '') + '>' + category + '</div>';
			item.fields = ['<div class="check img-check"></div>', status, item.data.time, name + dupe + dupeScore + repair + backup, category, item.data.age, item.data.size];
		}
		else
		{
			var info = '<div class="check img-check"></div><span class="row-title">' + name + '</span>' + dupe + dupeScore +
				' ' + status + repair + backup + ' <span class="label">' + item.data.time + '</span>';
			if (category)
			{
				info += ' <span class="label label-status text-uppercase">' + category + '</span>';
			}
			if (hist.Kind === 'NZB')
			{
				info += ' <span class="label">' + item.data.size + '</span>';
			}
			item.fields = [info];
		}
	}

	function renderCellCallback(cell, index, item)
	{
		if (index === 1)
		{
			cell.className = 'text-center' + (!UISettings.miniTheme ? ' dropdown-cell' : '');
		}
		else if (index === 4)
		{
			cell.className = !UISettings.miniTheme ? 'dropdown-cell' : '';
		}
		else if (index === 2)
		{
			cell.className = 'text-center';
		}
		else if (index === 6)
		{
			cell.className = 'text-right';
		}
	}

	this.recordsPerPageChange = function()
	{
		var val = $HistoryRecordsPerPage.val();
		UISettings.write('HistoryRecordsPerPage', val);
		$HistoryTable.fasttable('setPageSize', val);
	}

	function updateInfo(stat)
	{
		updateTabInfo($HistoryTabBadge, stat);
		if (activeTab)
		{
			updateFilterButtons();
		}
	}

	this.actionClick = function(action)
	{
		var checkedRows = $HistoryTable.fasttable('checkedRows');
		var checkedCount = $HistoryTable.fasttable('checkedCount');
		if (checkedCount === 0)
		{
			PopupNotification.show('#Notif_History_Select');
			return;
		}

		var pageCheckedCount = $HistoryTable.fasttable('pageCheckedCount');
		var checkedPercentage = Util.round0(checkedCount / history.length * 100);
		
		var hasNzb = false;
		var hasUrl = false;
		var hasDup = false;
		var hasFailed = false;
		for (var i = 0; i < history.length; i++)
		{
			var hist = history[i];
			if (checkedRows[hist.ID])
			{
				hasNzb |= hist.Kind === 'NZB';
				hasUrl |= hist.Kind === 'URL';
				hasDup |= hist.Kind === 'DUP';
				hasFailed |= hist.ParStatus === 'FAILURE' || hist.UnpackStatus === 'FAILURE' ||
					hist.DeleteStatus != 'NONE';
			}
		}

		switch (action)
		{
			case 'DELETE':
				notification = '#Notif_History_Deleted';
				HistoryUI.deleteConfirm(historyAction, hasNzb, hasDup, hasFailed, true, checkedCount, pageCheckedCount, checkedPercentage);
				break;

			case 'REPROCESS':
				if (hasUrl || hasDup)
				{
					PopupNotification.show('#Notif_History_CantReprocess');
					return;
				}
				notification = '#Notif_History_Reprocess';
				historyAction('HistoryProcess');
				break;

			case 'REDOWNLOAD':
				if (hasDup)
				{
					PopupNotification.show('#Notif_History_CantRedownload');
					return;
				}
				notification = '#Notif_History_Returned';
				ConfirmDialog.showModal('HistoryEditRedownloadConfirmDialog',
					function () { historyAction('HistoryRedownload') },
					function () { HistoryUI.confirmMulti(checkedCount > 1); },
					checkedCount);
				break;

			case 'MARKSUCCESS':
			case 'MARKGOOD':
			case 'MARKBAD':
				if (hasUrl)
				{
					PopupNotification.show('#Notif_History_CantMark');
					return;
				}
				notification = '#Notif_History_Marked';

				ConfirmDialog.showModal(action === 'MARKSUCCESS' ? 'HistoryEditSuccessConfirmDialog' :
					action === 'MARKGOOD' ? 'HistoryEditGoodConfirmDialog' : 'HistoryEditBadConfirmDialog',
					function () // action
					{
						historyAction(action === 'MARKSUCCESS' ? 'HistoryMarkSuccess' :
							action === 'MARKGOOD' ? 'HistoryMarkGood' :'HistoryMarkBad');
					},
					function (_dialog) // init
					{
						HistoryUI.confirmMulti(checkedCount > 1);
					},
					checkedCount
				);
				break;
		}
	}

	function historyAction(command)
	{
		Refresher.pause();

		var ids = buildContextIdList();

		RPC.call('editqueue', [command, '', ids], function()
		{
			editCompleted();
		});
	}

	this.clearClick = function()
	{
		RPC.call('history', [true], function(allHistory)
		{
			if (allHistory.length === 0)
			{
				PopupNotification.show('#Notif_History_Empty');
				return;
			}

			var hasNzb = false;
			var hasDup = false;
			var hasFailed = false;
			for (var i = 0; i < allHistory.length; i++)
			{
				var hist = allHistory[i];
				hasNzb |= hist.Kind === 'NZB';
				hasDup |= hist.Kind === 'DUP';
				// Only NZB records carry meaningful Par/Unpack/DeleteStatus; DUP/URL
				// rows lack DeleteStatus, which would otherwise spuriously trip != 'NONE'.
				hasFailed |= hist.Kind === 'NZB' && (hist.ParStatus === 'FAILURE' ||
					hist.UnpackStatus === 'FAILURE' || hist.DeleteStatus != 'NONE');
			}

			notification = '#Notif_History_Cleared';
			HistoryUI.deleteConfirm(
				function(command) { clearAction(command, allHistory); },
				hasNzb, hasDup, hasFailed, true,
				allHistory.length, 0, 100, true);
		});
	}

	function clearAction(command, allHistory)
	{
		Refresher.pause();

		var ids = [];
		for (var i = 0; i < allHistory.length; i++)
		{
			ids.push(allHistory[i].ID);
		}

		RPC.call('editqueue', [command, '', ids], function()
		{
			editCompleted();
		});
	}

	function editCompleted()
	{
		Refresher.update();
		if (notification)
		{
			PopupNotification.show(notification);
			notification = null;
		}
	}

	function editClick(e)
	{
		e.preventDefault();
		e.stopPropagation();

		var histid = $(this).attr('data-nzbid');
		var area = $(this).attr('data-area');
		$(this).blur();

		var hist = findHist(histid);
		if (hist == null)
		{
			return;
		}

		HistoryEditDialog.showModal(hist, area);
	}

	function findHist(nzbid)
	{
		for (var i=0; i<history.length; i++)
		{
			var gr = history[i];
			if (gr.NZBID == nzbid)
			{
				return gr;
			}
		}
		return null;
	}

	function buildContextIdList(hist)
	{
		var editIds = [];
		var checkedRows = $HistoryTable.fasttable('checkedRows');
		for (var id in checkedRows)
		{
			editIds.push(parseInt(id));
		}
		if (hist !== undefined && editIds.indexOf(hist.NZBID) === -1)
		{
			editIds = [hist.NZBID];
		}
		return editIds;
	}
	this.buildContextIdList = buildContextIdList;

	function statusClick(e)
	{
		e.preventDefault();
		e.stopPropagation();
		var hist = findHist($(this).attr('data-nzbid'));

		HistoryActionsMenu.showPopupMenu(hist, 'left',
			{ left: $(this).offset().left - 30, top: $(this).offset().top,
				width: $(this).width() + 30, height: $(this).outerHeight() - 2 },
			function(_notification) { notification = _notification; },
			editCompleted);
	}

	function categoryClick(e)
	{
		e.preventDefault();
		e.stopPropagation();
		DownloadsUI.fillCategoryMenu($CategoryMenu);

		var hist = findHist($(this).attr('data-nzbid'));
		var editIds = buildContextIdList(hist);
		$CategoryMenu.data('nzbids', editIds);
		DownloadsUI.updateContextWarning($CategoryMenu, editIds);
		$('i', $CategoryMenu).text('');
		$('li[data="' + hist.Category + '"] i', $CategoryMenu).text('done');

		Frontend.showPopupMenu($CategoryMenu, 'bottom-left',
			{ left: $(this).offset().left - 30, top: $(this).offset().top,
				width: $(this).width() + 30, height: $(this).outerHeight() - 2 });
	}

	function categoryMenuClick(e)
	{
		e.preventDefault();
		var category = $(this).parent().attr('data');
		var nzbids = $CategoryMenu.data('nzbids');
		notification = '#Notif_History_Changed';
		RPC.call('editqueue', ['HistorySetCategory', category, nzbids], editCompleted);
	}

	function filterCallback(item)
	{
		return !activeTab || curFilter === 'ALL' || item.data.FilterKind === curFilter;
	}

	function initFilterButtons()
	{
		Util.show($('#History_Badge_DUPE, #History_Badge_DUPE2').closest('.btn'), Options.option('DupeCheck') === 'yes');
	}

	function updateFilterButtons()
	{
		var countSuccess = 0;
		var countFailure = 0;
		var countDeleted = 0;
		var countDupe = 0;

		var data = $HistoryTable.fasttable('availableContent');

		for (var i=0; i < data.length; i++)
		{
			var hist = data[i].data;
			switch (hist.FilterKind)
			{
				case 'SUCCESS': countSuccess++; break;
				case 'FAILURE': countFailure++; break;
				case 'DELETED': countDeleted++; break;
				case 'DUPE': countDupe++; break;
			}
		}
		$('#History_Badge_ALL,#History_Badge_ALL2').text(countSuccess + countFailure + countDeleted + countDupe);
		$('#History_Badge_SUCCESS,#History_Badge_SUCCESS2').text(countSuccess);
		$('#History_Badge_FAILURE,#History_Badge_FAILURE2').text(countFailure);
		$('#History_Badge_DELETED,#History_Badge_DELETED2').text(countDeleted);
		$('#History_Badge_DUPE,#History_Badge_DUPE2').text(countDupe);

		$('#HistoryTab_Toolbar .history-filter').removeClass('btn-active');
		$('#History_Badge_' + curFilter + ',#History_Badge_' + curFilter + '2').closest('.history-filter').addClass('btn-active');
		$('#HistoryTab_Toolbar .badge').removeClass('badge-active');
		$('#History_Badge_' + curFilter + ',#History_Badge_' + curFilter + '2').addClass('badge-active');
	}

	this.filter = function(type)
	{
		curFilter = type;
		History.redraw(true);
	}

	this.dupClick = function()
	{
		showDup = !showDup;
		$('#History_Dup').toggleClass('btn-active', showDup);
		Refresher.update();
	}

	this.processShortcut = function(key)
	{
		// keyDown in index.js falls through between tab cases; ignore keys when History isn't active.
		if (!activeTab) return false;
		switch (key)
		{
			case 'D': case 'Delete': case 'Meta+Backspace': History.actionClick('DELETE'); return true;
			case 'C': History.clearClick(); return true;
			case 'P': History.actionClick('REPROCESS'); return true;
			case 'N': History.actionClick('REDOWNLOAD'); return true;
			case 'M': History.actionClick('MARKSUCCESS'); return true;
			case 'G': History.actionClick('MARKGOOD'); return true;
			case 'B': History.actionClick('MARKBAD'); return true;
			case 'A': History.filter('ALL'); return true;
			case 'S': History.filter('SUCCESS'); return true;
			case 'F': History.filter('FAILURE'); return true;
			case 'L': History.filter('DELETED'); return true;
			case 'U': History.filter('DUPE'); return true;
			case 'H': History.dupClick(); return true;
		}
		return $HistoryTable.fasttable('processShortcut', key);
	}

}(jQuery));


/*** FUNCTIONS FOR HTML GENERATION (also used from other modules) *****************************/

var HistoryUI = (new function($)
{
	'use strict';

	this.buildStatusText = function(hist)
	{
		var status = hist.Status;
		var total = status.substring(0, 7);
		var detail = status.substring(8, 100);
		switch (total)
		{
			case 'SUCCESS':
				return detail === 'GOOD' ? I18n.translate('status_good') : I18n.translate('status_success');
			case 'FAILURE':
				return detail === 'BAD' ? I18n.translate('status_bad') : (status === 'FAILURE/INTERNAL_ERROR' ? I18n.translate('label_internal_error') : I18n.translate('status_failure'));
			case 'WARNING':
				if (detail === 'SCRIPT') return I18n.translate('status_pp_failure');
				if (detail === 'DAMAGED') return I18n.translate('status_manual');
				if (detail === 'REPAIRABLE') return I18n.translate('status_repairable');
				if (detail === 'HEALTH') return DownloadsUI.healthText(hist);
				if (detail === 'SPACE') return I18n.translate('status_space');
				if (detail === 'PASSWORD') return I18n.translate('status_password');
				if (detail === 'SKIPPED') return I18n.translate('status_skipped');
				return detail;
			case 'DELETED':
			{
				if (detail === 'COPY') return I18n.translate('status_copy');
				if (detail === 'MANUAL') return I18n.translate('btn_history_filter_deleted');
				if (detail === 'DUPE')
				{
					// a twin or an alt of the primary of its dupe key (TwinCheck)
					var kind = hist.Kind === 'NZB' ? DownloadsUI.parameterValue(hist.Parameters, 'DupeKind') : '';
					var label = DupeGroup.kindLabel(kind);
					return label === 'twin' ? I18n.translate('status_twin') :
						label === 'alt' ? I18n.translate('status_alt') :
						label === 'unverified' ? I18n.translate('status_unverified') : I18n.translate('status_dupe');
				}
				if (detail === 'GOOD') return I18n.translate('status_good');
				if (detail === 'SUCCESS') return I18n.translate('status_success');
				if (detail === 'HEALTH') return I18n.translate('status_health_val');
				if (detail === 'BAD') return I18n.translate('status_bad');
				if (detail === 'SCAN') return I18n.translate('status_scan_val');
				return detail;
			}
			default:
				return I18n.translate('label_internal_error') + ' (' + status + ')';
		}
	}

	this.buildStatus = function(hist)
	{
		var total = hist.Status.substring(0, 7);
		var statusText = HistoryUI.buildStatusText(hist);
		var badgeClass = '';
		switch (total)
		{
			case 'SUCCESS':
				badgeClass = 'label-success'; break;
			case 'FAILURE':
				badgeClass = 'label-important'; break;
			case 'WARNING':
				badgeClass = 'label-warning'; break;
		}
		return '<span class="label label-status ' + badgeClass + ' text-uppercase">' + statusText + '</span>';
	}

	this.deleteConfirm = function(actionCallback, hasNzb, hasDup, hasFailed, multi, selCount, pageSelCount, selPercentage, clearAll)
	{
		var dupeCheck = Options.option('DupeCheck') === 'yes';
		var dialog = null;

		function init(_dialog)
		{
			dialog = _dialog;
			if (clearAll)
			{
				var html = $('#ConfirmDialog_Text').html();
				html = html.replace(/Selected/g, 'All').replace(/selected/g, 'all');
				$('#ConfirmDialog_Text').html(html);
				$('#ConfirmDialog_OK').text(I18n.translate('btn_history_clear'));
				// Clear is always permanent: HistoryDelete on hidden DUPs is a no-op
				// and on visible NZBs it just creates new hidden DUPs, leaving rows behind.
				Util.show($('#HistoryDeleteConfirmDialog_Options', dialog), false);
				Util.show($('#HistoryDeleteConfirmDialog_Simple', dialog), true);
				Util.show($('#HistoryDeleteConfirmDialog_DeleteWillCleanup', dialog), hasFailed);
				Util.show($('#HistoryDeleteConfirmDialog_DeleteNoCleanup', dialog), !hasFailed);
				Util.show($('#HistoryDeleteConfirmDialog_DupAlert', dialog), dupeCheck && hasDup);
				Util.show('#ConfirmDialog_Help', false);
				HistoryUI.confirmMulti(multi);
				return;
			}
			HistoryUI.confirmMulti(multi);
			$('#HistoryDeleteConfirmDialog_Hide', dialog).prop('checked', true);
			Util.show($('#HistoryDeleteConfirmDialog_Options', dialog), hasNzb && dupeCheck);
			Util.show($('#HistoryDeleteConfirmDialog_Simple', dialog), !(hasNzb && dupeCheck));
			Util.show($('#HistoryDeleteConfirmDialog_DeleteWillCleanup', dialog), hasNzb && hasFailed);
			Util.show($('#HistoryDeleteConfirmDialog_DeleteNoCleanup', dialog), !(hasNzb && hasFailed));
			Util.show($('#HistoryDeleteConfirmDialog_DupAlert', dialog), !hasNzb && dupeCheck && hasDup);
			Util.show('#ConfirmDialog_Help', hasNzb && dupeCheck);
		};

		function action()
		{
			var command;
			if (clearAll)
			{
				command = 'HistoryFinalDelete';
			}
			else
			{
				var hide = $('#HistoryDeleteConfirmDialog_Hide', dialog).is(':checked');
				command = hasNzb && hide ? 'HistoryDelete' : 'HistoryFinalDelete';
			}
			if (selCount - pageSelCount > 0 && selCount >= 50)
			{
				PurgeHistoryDialog.showModal(function(){actionCallback(command);}, selCount, selPercentage);
			}
			else
			{
				actionCallback(command);
			}
		}

		ConfirmDialog.showModal('HistoryDeleteConfirmDialog', action, init, clearAll ? 1 : selCount);
	}

	this.confirmMulti = function(multi)
	{
		if (multi === undefined || !multi)
		{
			var html = $('#ConfirmDialog_Text').html();
			html = html.replace(/records/g, 'record');
			html = html.replace(/nzbs/g, 'nzb');
			$('#ConfirmDialog_Text').html(html);
		}
	}

	// The repair record the daemon keeps in the RepairStats parameter (see
	// rust/src/repairsummary.rs): how much was repaired, and how. Null for
	// entries that finished before it existed, or that never reached history.
	var DONORS = ['twin', 'alt', 'container', 'other', 'unlabelled'];
	var WAYS = ['live', 'par'].concat(DONORS, ['payload']);

	this.repairRecord = function(hist)
	{
		var value = hist.Kind === 'NZB' ? DownloadsUI.parameterValue(hist.Parameters, 'RepairStats') : null;
		if (!value)
		{
			return null;
		}
		var r = { live: [0, 0, 0], par: [0, 0, 0], payload: [0, 0, 0], readback: 0, twin: 0, alt: 0, container: 0,
			other: 0, unlabelled: 0, volumes: 0, faildata: 0, failpar: 0, sec: 0, result: '', method: '' };
		var num = function(v) { var n = parseInt(v, 10); return isFinite(n) ? n : 0; };
		String(value).split(';').forEach(function(pair)
		{
			var eq = pair.indexOf('=');
			if (eq < 0) return;
			var key = pair.substring(0, eq).trim(), val = pair.substring(eq + 1).trim();
			if (key === 'live' || key === 'par' || key === 'payload') r[key] = val.split('/').concat([0, 0, 0]).slice(0, 3).map(num);
			else if (key === 'method' || key === 'result') r[key] = val;
			else if (r.hasOwnProperty(key)) r[key] = num(val);
		});
		if (!r.method)
		{
			return null; // still queued: not settled yet
		}
		// only the names the daemon writes; anything else is dropped
		if (/^(clean|par2only|unrepaired)$/.test(r.method))
		{
			r.methods = [];
		}
		else
		{
			r.methods = r.method.split('+').map(function(m) { return m.trim(); }).filter(function(m, i, all) {
				return WAYS.indexOf(m) > -1 && all.indexOf(m) === i;
			});
			if (!r.methods.length) return null;
			r.method = r.methods.join('+');
		}
		r.failed = r.result === 'failed';
		r.slices = Math.max(0, r.live[0]) + Math.max(0, r.par[0]);
		r.swapped = DONORS.reduce(function(sum, m) { return sum + Math.max(0, r[m]); }, 0);
		r.borrowed = r.swapped + Math.max(0, r.payload[0]);
		return r;
	};

	// "$1 slices" / "1 slice": keys with an _one variant for a count of one.
	function count(key, n)
	{
		return I18n.translate(n === 1 ? key + '_one' : key, String(n));
	}

	function repairKind(r)
	{
		if (r.method === 'unrepaired' || r.failed) return 'failed';
		if (r.method === 'clean' || r.method === 'par2only') return 'none';
		if (r.methods.length > 1) return 'mixed';
		return r.methods[0] === 'live' ? 'live' : r.methods[0] === 'par' ? 'par' : 'borrow';
	}

	function borrowSource(m)
	{
		return I18n.translate('repair_from_' + m);
	}

	function mb(bytes)
	{
		return Util.formatSizeMB(Math.floor(bytes / 1048576), bytes % 4294967296);
	}

	// What the ways repaired, shortest first: slices, articles, video bytes.
	function amounts(r)
	{
		var out = [];
		if (r.slices > 0) out.push(count('repair_slices', r.slices));
		if (r.borrowed > 0) out.push(count('repair_articles', r.borrowed));
		else if (r.payload[1] > 0) out.push(mb(r.payload[1]));
		return out.join(', ');
	}

	function repairWays(r)
	{
		if (r.methods.length > 1)
		{
			var names = r.methods.map(function(m) { return I18n.translate('repair_short_' + m); }).join(' + ');
			var much = amounts(r);
			return I18n.translate('repair_mixed', names) + (much ? ' · ' + much : '');
		}
		var m = r.methods[0];
		if (m === 'live') return I18n.translate('repair_live', count('repair_slices', r.live[0]));
		if (m === 'par') return I18n.translate('repair_par', count('repair_slices', r.par[0]));
		if (m === 'payload') return I18n.translate('repair_payload', amounts(r) || mb(0));
		return I18n.translate('repair_borrowed', count('repair_articles', Math.max(0, r[m])), borrowSource(m));
	}

	// One line for the overview: what happened and how much.
	this.repairText = function(r)
	{
		switch (r.method)
		{
			case 'clean': return I18n.translate('repair_not_needed');
			case 'par2only': return I18n.translate('repair_nothing');
			case 'unrepaired': return I18n.translate('repair_not_repaired');
		}
		var text = repairWays(r);
		return r.failed ? I18n.translate('repair_incomplete', text) : text;
	};

	// The overview badge; empty for entries without a repair record.
	this.buildRepairBadge = function(hist)
	{
		var r = HistoryUI.repairRecord(hist);
		if (!r)
		{
			return '';
		}
		var kind = repairKind(r);
		var text = HistoryUI.repairText(r);
		var title = r.method === 'par2only' ? I18n.translate('repair_nothing_title') : text;
		return ' <span class="label repair-badge repair-badge--' + kind + '" title="' + Util.textToAttr(title) + '">' +
			Util.textToHtml(text) + '</span>';
	};

	function bar(used, available)
	{
		if (!(available > 0))
		{
			return '';
		}
		var pct = Math.max(0, Math.min(100, Math.round(used * 100 / available)));
		return '<div class="repair-bar" role="img" aria-label="' + Util.textToAttr(I18n.translate('repair_blocks', String(used), String(available))) +
			'"><div class="repair-bar__fill" style="width:' + pct + '%"></div></div>';
	}

	// The Repair section of the details panel.
	this.buildRepairSection = function(hist)
	{
		var r = HistoryUI.repairRecord(hist);
		if (!r)
		{
			return '';
		}
		var rows = [];
		var row = function(label, value, extra)
		{
			rows.push('<div class="repair-row"><span class="repair-row__label">' + Util.textToHtml(label) + '</span>' +
				'<span class="repair-row__value">' + Util.textToHtml(value) + '</span>' + (extra || '') + '</div>');
		};
		row(I18n.translate('repair_damaged'), I18n.translate('repair_damaged_value', mb(r.faildata), mb(r.failpar)));
		if (r.live[0] > 0)
		{
			row(I18n.translate('repair_how_live'), count('repair_slices', r.live[0]) +
				(r.live[2] > 0 ? ' · ' + I18n.translate('repair_blocks', String(r.live[1]), String(r.live[2])) : ''), bar(r.live[1], r.live[2]));
		}
		if (r.par[0] > 0)
		{
			row(I18n.translate('repair_how_par'), count('repair_slices', r.par[0]) +
				(r.par[2] > 0 ? ' · ' + I18n.translate('repair_blocks', String(r.par[0]), String(r.par[2])) : ''), bar(r.par[0], r.par[2]));
		}
		var donors = DONORS.filter(function(m) { return r[m] > 0; });
		if (donors.length)
		{
			row(I18n.translate('repair_borrowed_label'), donors.map(function(m) {
				return count('repair_articles', r[m]) + ' ' + borrowSource(m);
			}).join(' · '));
		}
		if (r.payload[0] > 0 || r.payload[1] > 0 || r.payload[2] > 0 || r.volumes > 0)
		{
			var parts = [mb(r.payload[1])];
			if (r.payload[0] > 0) parts.push(count('repair_articles', r.payload[0]));
			if (r.payload[2] > 0) parts.push(count('repair_holes', r.payload[2]));
			if (r.volumes > 0) parts.push(count('repair_volumes', r.volumes));
			row(I18n.translate('repair_payload_label'), parts.join(' · '));
		}
		if (r.failed)
		{
			row(I18n.translate('repair_result'), I18n.translate('repair_result_failed'));
		}
		if (r.sec > 0)
		{
			row(I18n.translate('stat_repair_time'), Util.formatTimeHMS(r.sec));
		}
		if (r.readback > 0)
		{
			row(I18n.translate('repair_readback'), mb(r.readback));
		}
		return '<div class="repair-head"><span class="repair-title" id="HistoryEdit_RepairTitle">' + Util.textToHtml(I18n.translate('repair_title')) + '</span>' +
			HistoryUI.buildRepairBadge(hist) + '</div>' + rows.join('');
	};

}(jQuery));

/*** HISTORY ACTION MENU *************************************************************************/

var HistoryActionsMenu = (new function($)
{
	'use strict'

	var $ActionsMenu;
	var curHist;
	var beforeCallback;
	var completedCallback;
	var editIds;

	this.init = function()
	{
		$ActionsMenu = $('#HistoryActionsMenu');
		$('#HistoryActions_Delete').click(itemDelete);
		$('#HistoryActions_Return, #HistoryActions_ReturnURL').click(itemReturn);
		$('#HistoryActions_Reprocess').click(itemReprocess);
		$('#HistoryActions_Redownload').click(itemRedownload);
		$('#HistoryActions_RetryFailed').click(itemRetryFailed);
		$('#HistoryActions_MarkSuccess').click(itemSuccess);
		$('#HistoryActions_MarkGood').click(itemGood);
		$('#HistoryActions_MarkBad').click(itemBad);
	}

	this.showPopupMenu = function(hist, anchor, rect, before, completed)
	{
		curHist = hist;
		beforeCallback = before;
		completedCallback = completed;
		editIds = History.buildContextIdList(hist);

		// setup menu items
		Util.show('#HistoryActions_Return', hist.RemainingFileCount > 0);
		Util.show('#HistoryActions_ReturnURL', hist.Kind === 'URL');
		Util.show('#HistoryActions_Redownload, #HistoryActions_Reprocess', hist.Kind === 'NZB');
		Util.show('#HistoryActions_RetryFailed', hist.Kind === 'NZB' && hist.FailedArticles > 0 && hist.RetryData);
		var dupeCheck = Options.option('DupeCheck') === 'yes';
		Util.show('#HistoryActions_MarkSuccess', dupeCheck && ((hist.Kind === 'NZB' && hist.MarkStatus !== 'SUCCESS') || (hist.Kind === 'DUP' && hist.DupStatus !== 'SUCCESS')) &&
			hist.Status.substr(0, 7) !== 'SUCCESS');
		Util.show('#HistoryActions_MarkGood', dupeCheck && ((hist.Kind === 'NZB' && hist.MarkStatus !== 'GOOD') || (hist.Kind === 'DUP' && hist.DupStatus !== 'GOOD')));
		Util.show('#HistoryActions_MarkBad', dupeCheck && hist.Kind !== 'URL');
		DownloadsUI.updateContextWarning($ActionsMenu, editIds);

		DownloadsUI.buildDNZBLinks(hist.Parameters ? hist.Parameters : [], 'HistoryActions_DNZB');
		
		Frontend.showPopupMenu($ActionsMenu, anchor, rect);
	}
	
	function itemDelete(e)
	{
		e.preventDefault();
		HistoryUI.deleteConfirm(doItemDelete, curHist.Kind === 'NZB', curHist.Kind === 'DUP',
			curHist.ParStatus === 'FAILURE' || curHist.UnpackStatus === 'FAILURE' ||
			curHist.DeleteStatus != 'NONE', false);
	}

	function execAction(command, notification)
	{
		function performAction()
		{
			RPC.call('editqueue', [command, '', editIds], completedCallback);
		}

		var async = beforeCallback(notification, performAction);
		if (!async)
		{
			performAction();
		}
	}

	function doItemDelete(command)
	{
		execAction(command, '#Notif_History_Deleted');
	}

	function itemReturn(e)
	{
		e.preventDefault();
		execAction('HistoryReturn', '#Notif_History_Returned');
	}

	function itemRedownload(e)
	{
		e.preventDefault();
		if (curHist.SuccessArticles > 0)
		{
			ConfirmDialog.showModal('HistoryEditRedownloadConfirmDialog', doItemRedownload,
				function () { HistoryUI.confirmMulti(false); });
		}
		else
		{
			doItemRedownload();
		}
	}

	function doItemRedownload()
	{
		execAction('HistoryRedownload', '#Notif_History_Returned');
	}

	function itemReprocess(e)
	{
		e.preventDefault();
		execAction('HistoryProcess', '#Notif_History_Reprocess');
	}

	function itemRetryFailed(e)
	{
		e.preventDefault();
		execAction('HistoryRetryFailed', '#Notif_History_RetryFailed');
	}

	function itemSuccess(e)
	{
		e.preventDefault();
		ConfirmDialog.showModal('HistoryEditSuccessConfirmDialog', doItemSuccess,
			function () { HistoryUI.confirmMulti(editIds.length > 1); });
	}

	function doItemSuccess()
	{
		execAction('HistoryMarkSuccess', '#Notif_History_Marked');
	}

	function itemGood(e)
	{
		e.preventDefault();
		ConfirmDialog.showModal('HistoryEditGoodConfirmDialog', doItemGood,
			function () { HistoryUI.confirmMulti(editIds.length > 1); });
	}

	function doItemGood()
	{
		execAction('HistoryMarkGood', '#Notif_History_Marked');
	}

	function itemBad(e)
	{
		e.preventDefault();
		ConfirmDialog.showModal('HistoryEditBadConfirmDialog', doItemBad,
			function () { HistoryUI.confirmMulti(editIds.length > 1); });
	}

	function doItemBad()
	{
		execAction('HistoryMarkBad', '#Notif_History_Marked');
	}
}(jQuery));

/*** PURGE HISTORY DIALOG *****************************************************/

var PurgeHistoryDialog = (new function($)
{
	'use strict';

	// Controls
	var $PurgeHistoryDialog;

	// State
	var actionCallback;

	this.init = function()
	{
		$PurgeHistoryDialog = $('#PurgeHistoryDialog');
	}

	this.showModal = function(_actionCallback, count, percentage)
	{
		actionCallback = _actionCallback;
		$('#PurgeHistoryDialog_count,#PurgeHistoryDialog_count2').text(count);
		$('#PurgeHistoryDialog_percentage').text(percentage);
		Util.centerDialog($PurgeHistoryDialog, true);
		$PurgeHistoryDialog.modal({backdrop: 'static'});
	}

	this.delete = function(event)
	{
		event.preventDefault(); // avoid scrolling
		$PurgeHistoryDialog.modal('hide');
		actionCallback();
	}
}(jQuery));
