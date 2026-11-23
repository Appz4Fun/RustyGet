/*
 * This file is part of nzbget. See <https://nzbget.com>.
 *
 * Copyright (C) 2012-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 * Copyright (C) 2025-2026 Denis <denis@nzbget.com>
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
 *   1) File upload dialog (local files, urls, scan);
 *   2) Drag-n-drop events handling on main page.
 */
 
/*** FILE UPLOAD (DRAG-N-DROP, URLS, SCAN) ************************************************/
 
var Upload = (new function($)
{
	'use strict';

	// Controls
	var $AddDialog;
	
	// State
	var dragin = false;
	var files = [];
	var infos = [];
	var filesSuccess = [];
	var index;
	var errors = false;
	var needRefresh = false;
	var failure_message = null;
	var url = '';
	var urlInfo;
	// one fleet per group of copies of a release (docs/DROP_AS_FLEET.md)
	var jobs = [];
	var jobIndex = 0;
	var droppedCount = 0;
	var contents = [];

	this.init = function()
	{
		var target = $('#DownloadsTab')[0];
		if (target)
		{
			target.addEventListener('dragenter', bodyDragOver);
			target.addEventListener('dragover', bodyDragOver);
			target.addEventListener('dragleave', bodyDragLeave);
			target.addEventListener('drop', bodyDrop, false);
		}

		target = $('#AddDialog_Target')[0];
		if (target)
		{
			target.addEventListener('dragenter', dialogDragOver);
			target.addEventListener('dragover', dialogDragOver);
			target.addEventListener('dragleave', dialogDragLeave);
			target.addEventListener('drop', dialogDrop, false);
		}

		$AddDialog = $('#AddDialog');

		$AddDialog.on('hidden', function ()
		{
			Refresher.resume();
			files = [];
			infos = [];
			filesSuccess = [];
			if (needRefresh)
			{
				Refresher.update();
			}
		});

		if (UISettings.setFocus)
		{
			$AddDialog.on('shown', function ()
			{
				if (files.length === 0)
				{
					$('#AddDialog_URL').focus();
				}
			});
		}

		// Workaround for iPadOS/iOS: The native Document Picker fails to map custom extensions
		// in the accept attribute (like .nzb or .7z), graying out valid files.
		var isIOS = /iPad|iPhone|iPod/.test(navigator.platform) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
		if (isIOS)
		{
			$('#AddDialog_Input').removeAttr('accept');
		}

		$('#AddDialog_Select').click(selectFiles);
		$('#AddDialog_Submit').click(submit);
		var inp = $('#AddDialog_Input')[0];
		if (inp)
		{
			inp.addEventListener("change", fileSelectHandler, false);
		}
		$('#AddDialog_Scan').click(scan);
		$('#AddDialog_Fleet, #AddDialog_DupeForce, #AddDialog_Paused, #AddDialog_AutoCategory').change(updateFleetPlan);
		
		AddParamDialog.init();
	}

	function bodyDragOver(event)
	{
		if ((event.dataTransfer.types.contains && event.dataTransfer.types.contains('Files')) ||
			(event.dataTransfer.types.indexOf && event.dataTransfer.types.indexOf('Files') > -1) ||
			(event.dataTransfer.files && event.dataTransfer.files.length > 0))
		{
			event.stopPropagation();
			event.preventDefault();

			if (!dragin)
			{
				dragin = true;
				$('body').addClass('dragover');
			}
		}
	}

	function bodyDragLeave(event)
	{
		dragin = false;
		$('body').removeClass('dragover');
	}

	function bodyDrop(event)
	{
		event.preventDefault();
		bodyDragLeave();

		if (!event.dataTransfer.files)
		{
			showDnDUnsupportedAlert();
			return;
		}

		showModal(event.dataTransfer.files);
	}

	function dialogDragOver(event)
	{
		event.stopPropagation();
		event.preventDefault();

		if (!dragin)
		{
			dragin = true;
			$('#AddDialog_Target').addClass('dragover');
		}
	}

	function dialogDragLeave(event)
	{
		dragin = false;
		$('#AddDialog_Target').removeClass('dragover');
	}

	function dialogDrop(event)
	{
		event.preventDefault();
		dialogDragLeave();

		if (!event.dataTransfer.files)
		{
			showDnDUnsupportedAlert();
			return;
		}

		addFiles(event.dataTransfer.files);
	}

	function showDnDUnsupportedAlert()
	{
		setTimeout(function()
		{
			alert(I18n.translate('msg_dnd_not_supported'));
		}, 50);
	}

	function selectFiles()
	{
		var inp = $('#AddDialog_Input');

		// Reset file input control
		inp.val('');		

		inp.click();
	}

	function fileSelectHandler(event)
	{
		if (!event.target.files)
		{
			alert(I18n.translate('msg_no_direct_file_access_add'));
			return;
		}
		addFiles(event.target.files);
	}

	function addFiles(selectedFiles)
	{
		for (var i = 0; i<selectedFiles.length; i++)
		{
			var file = selectedFiles[i];
			var filename = file.name.replace(/\.queued$/g, '');
			var html = '<a class="link-black" href="#" onclick="Upload.renameClick(' + files.length + ')" title="' + I18n.translate('label_click_to_rename') + '">'+
				'<table><tr><td width="18px" valign="top">'+
				'<i class="material-icon material-icon--draft">draft</i>'+
				'<i class="material-icon material-icon--success hide">check_circle</i>'+
				'<i class="material-icon material-icon--error hide">error</i>'+
				'<i class="material-icon material-icon--progress spinner hide">progress_activity</i>'+
				'</td><td id="AddDialog_File' + files.length + '">' + Util.textToHtml(Util.formatNZBName(filename)) + '</td></tr></table></a>';
			$('#AddDialog_Files').append(html);
			files.push(file);

			var p = filename.lastIndexOf('.');
			var name = p > -1 ? filename.substr(0, p) : filename;
			var ext = p > -1 ? filename.substr(filename.lastIndexOf('.')) : '';
			var info = { name: name, ext: ext, password: '', dupekey: '', dupescore: 0 };
			infos.push(info);
		}
		$('#AddDialog_Files,#AddDialog_RenameTip').show();
		$('#AddDialog_FilesHelp').hide();
		updateFleetPlan();
	}

	function fleetMode()
	{
		// a fleet has no per-copy options: with Add paused or Auto-category the
		// files go in on their own, honouring them, as before
		var plain = $('#AddDialog_Paused').is(':checked') || $('#AddDialog_AutoCategory').is(':checked');
		return { enabled: $('#AddDialog_Fleet').is(':checked') && !plain, force: $('#AddDialog_DupeForce').is(':checked') };
	}

	function planFiles(withContents)
	{
		var list = [];
		for (var i = 0; i < files.length; i++)
		{
			list.push({
				name: infos[i].name + infos[i].ext,
				// a file with a password goes in on its own: a fleet has no per-copy parameters
				content: withContents ? contents[i] : undefined,
				dupekey: infos[i].dupekey
			});
		}
		var plan = DupeGroup.plan(list, fleetMode());
		// copies with a password leave their fleet
		for (var f = plan.fleets.length - 1; f >= 0; f--)
		{
			var fleet = plan.fleets[f];
			fleet.members = fleet.members.filter(function(m)
			{
				// a password or a DupeScore of its own: the file goes in on its own
				if (infos[m].password === '' && !infos[m].dupescore) return true;
				plan.singles.push(m);
				return false;
			});
			if (fleet.members.length < 2)
			{
				plan.singles = plan.singles.concat(fleet.members);
				plan.fleets.splice(f, 1);
			}
		}
		plan.singles.sort(function(a, b) { return a - b; });
		return plan;
	}

	// The grouping, shown before Submit: "19 files → 1 fleet: Dune.Part.Two…FraMeSToR".
	function updateFleetPlan()
	{
		var $plan = $('#AddDialog_FleetPlan');
		$('#AddDialog_FleetRow').toggle(files.length > 1);
		var plan = files.length > 1 ? planFiles(false) : { fleets: [] };
		if (plan.fleets.length === 0)
		{
			$plan.empty().hide();
			return;
		}
		var html = '';
		for (var i = 0; i < plan.fleets.length; i++)
		{
			var fleet = plan.fleets[i];
			html += '<div class="add-dialog-fleet">' + Util.textToHtml(
				I18n.translate('add_dialog_fleet_plan', fleet.members.length, fleet.title)) + '</div>';
		}
		$plan.html(html).show();
	}

	this.addClick = function()
	{
		showModal();
	}
	
	this.renameClick = function(no)
	{
		var info = infos[no];
		AddParamDialog.showModal(info, function() {
			$('#AddDialog_File' + no).html(Util.textToHtml(Util.formatNZBName(info.name + info.ext)));
			updateFleetPlan();
		});
	}

	this.renameURLClick = function(no)
	{
		AddParamDialog.showModal(urlInfo, function(){});
	}

	function showModal(droppedFiles)
	{
		Refresher.pause();

		$('#AddDialog_Files').empty();
		$('#AddDialog_URL').val('');
		$('#AddDialog_FilesHelp').show();
		$('#AddDialog_URLLabel img').hide();
		$('#AddDialog_URLLabel i').hide();
		$('#AddDialog_AutoCategory').prop('checked', false);
		$('#AddDialog_Paused').prop('checked', false);
		$('#AddDialog_DupeForce').prop('checked', false);
		$('#AddDialog_Fleet').prop('checked', true);
		$('#AddDialog_FleetPlan').empty().hide();
		$('#AddDialog_FleetRow').hide();
		enableAllButtons();

		var v = $('#AddDialog_Priority');
		DownloadsUI.fillPriorityCombo(v);
		v.val(0);
		DownloadsUI.fillCategoryCombo($('#AddDialog_Category'));

		files = [];
		infos = [];
		contents = [];
		filesSuccess = [];
		urlInfo = { name: '', password: '', dupekey: '', dupescore: 0 };

		if (droppedFiles)
		{
			addFiles(droppedFiles);
		}

		$AddDialog.modal({backdrop: 'static'});
	}

	function disableAllButtons()
	{
		$('#AddDialog .modal-footer .btn, #AddDialog_Select, #AddDialog_Scan').attr('disabled', 'disabled');
	}

	function enableAllButtons()
	{
		$('#AddDialog .modal-footer .btn, #AddDialog_Select, #AddDialog_Scan').removeAttr('disabled');
		$('#AddDialog_Transmit').hide();
	}

	function submit()
	{
		disableAllButtons();

		if (files.length > 0)
		{
			if (!window.FileReader)
			{
				$AddDialog.modal('hide');
				alert(I18n.translate('msg_no_filereader_api_add'));
				return;
			}

			var testreader = new FileReader();
			if (!testreader.readAsBinaryString && !testreader.readAsDataURL)
			{
				$AddDialog.modal('hide');
				alert(I18n.translate('msg_no_filereader_api_detailed'));
				return;
			}
		}

		needRefresh = false;
		errors = false;
		failure_message = null;
		index = 0;
		url = $('#AddDialog_URL').val();
		url = url.trim();

	/*
		setTimeout(function(){
			$('#AddDialog_Transmit').show();
		}, 500);
	*/

		if (url.length > 0)
		{
			urlNext();
		}
		else
		{
			filesStart();
		}
	}

	// Files go in one by one as before, unless copies of one release are to be
	// one fleet: then every file is read first, so exact copies are dropped
	// and each group goes in with one appendfleet call.
	function filesStart()
	{
		if (files.length < 2 || !fleetMode().enabled || fleetMode().force)
		{
			fileNext();
			return;
		}
		contents = [];
		droppedCount = 0;
		readAll(0);
	}

	function readAll(no)
	{
		if (no === files.length)
		{
			startJobs();
			return;
		}
		var reader = new FileReader();
		reader.onload = function (event)
		{
			var result = event.target.result;
			contents[no] = result.slice(result.indexOf(',') + 1);
			readAll(no + 1);
		};
		reader.onerror = function ()
		{
			contents[no] = null;
			readAll(no + 1);
		};
		reader.readAsDataURL(files[no]);
	}

	function startJobs()
	{
		var plan = planFiles(true);
		jobs = [];
		jobIndex = 0;
		for (var d = 0; d < plan.dropped.length; d++)
		{
			// an exact copy of another file: nothing to send, and the dialog says so
			var no = plan.dropped[d].index;
			var same = plan.dropped[d].sameAs;
			filesSuccess.push(files[no]);
			setFileIcon(no, 'success');
			$('#AddDialog_File' + no).append(' <span class="add-dialog-dropped">' +
				Util.textToHtml(I18n.translate('add_dialog_exact_copy', infos[same].name + infos[same].ext)) + '</span>');
			droppedCount++;
		}
		var fleetStart = {};
		for (var f = 0; f < plan.fleets.length; f++)
		{
			fleetStart[plan.fleets[f].members[0]] = plan.fleets[f];
		}
		var inFleet = {};
		plan.fleets.forEach(function(fleet) { fleet.members.forEach(function(m) { inFleet[m] = true; }); });
		for (var i = 0; i < files.length; i++)
		{
			if (fleetStart[i])
			{
				jobs.push({ fleet: fleetStart[i] });
			}
			else if (!inFleet[i] && plan.singles.indexOf(i) > -1)
			{
				jobs.push({ single: i });
			}
		}
		jobNext();
	}

	function setFileIcon(no, state)
	{
		var $t = $('#AddDialog_Files table:eq(' + no + ')');
		$t.find('i.material-icon').addClass('hide');
		$t.find('i.material-icon--' + state).removeClass('hide');
	}

	function jobNext()
	{
		if (jobIndex === jobs.length)
		{
			index = files.length;
			allCompleted();
			return;
		}
		var job = jobs[jobIndex++];
		if (job.single !== undefined)
		{
			var no = job.single;
			if (filesSuccess.indexOf(files[no]) > -1)
			{
				jobNext();
				return;
			}
			setFileIcon(no, 'progress');
			appendFile(no, contents[no], function(result)
			{
				var failure = result < 0 || (result == 0 && Options.option('ScanScript') === '');
				fileDone(no, failure);
				jobNext();
			});
			return;
		}
		var fleet = job.fleet;
		var category = $('#AddDialog_Category').val();
		var priority = parseInt($('#AddDialog_Priority').val());
		var params = [fleet.key, category, priority, 60];
		fleet.members.forEach(function(m)
		{
			setFileIcon(m, 'progress');
			params.push(infos[m].name + infos[m].ext, contents[m]);
		});
		RPC.call('appendfleet', params, function(result)
		{
			var members = (result && result.Members) || [];
			var used = [];
			fleet.members.forEach(function(m)
			{
				// the reply echoes each name without its {{password}}
				var name = DupeGroup.withoutPassword(infos[m].name + infos[m].ext);
				var status = 'ERROR';
				for (var k = 0; k < members.length; k++)
				{
					if (used.indexOf(k) < 0 && members[k].Name === name)
					{
						used.push(k);
						status = members[k].Status;
						break;
					}
				}
				fileDone(m, status === 'ERROR');
			});
			jobNext();
		}, function(res)
		{
			failure_message = res;
			fleet.members.forEach(function(m) { fileDone(m, true); });
			jobNext();
		});
	}

	function fileDone(no, failure)
	{
		errors |= failure;
		needRefresh |= !failure;
		if (!failure)
		{
			filesSuccess.push(files[no]);
		}
		setFileIcon(no, failure ? 'error' : 'success');
	}

	function appendFile(no, base64str, done)
	{
		var info = infos[no];
		var category = $('#AddDialog_Category').val();
		var priority = parseInt($('#AddDialog_Priority').val());
		var filename = info.name + info.ext;
		var addPaused = $('#AddDialog_Paused').is(':checked');
		var autoCategory = $('#AddDialog_AutoCategory').is(':checked');
		var dupeMode = $('#AddDialog_DupeForce').is(':checked') ? "FORCE" : "SCORE";
		var params = info.password === '' ? [] : [{'*Unpack:Password' : info.password}];
		RPC.call('append', [
			filename,
			base64str,
			category,
			priority,
			false,
			addPaused,
			info.dupekey,
			info.dupescore,
			dupeMode,
			autoCategory,
			params,
		], done, function(res)
		{
			failure_message = res;
			done(-1);
		});
	}

	function fileNext()
	{
		if (index === files.length)
		{
			allCompleted();
			return;
		}

		var file = files[index];
		var info = infos[index];

		if (filesSuccess.indexOf(file) > -1)
		{
			// file already uploaded
			index++;
			setTimeout(next, 50);
			return;
		}

		$('#AddDialog_Files table:eq(' + index + ') i.material-icon--progress').toggleClass('hide');
		$('#AddDialog_Files table:eq(' + index + ') i.material-icon--draft').toggleClass('hide');

		var reader = new FileReader();
		reader.onload = function (event)
		{
			var result = event.target.result;
			var base64str = result.slice(result.indexOf(',') + 1);
			var category = $('#AddDialog_Category').val();
			var priority = parseInt($('#AddDialog_Priority').val());
			var filename = info.name + info.ext;
			var addPaused = $('#AddDialog_Paused').is(':checked');
			var autoCategory = $('#AddDialog_AutoCategory').is(':checked');
			var dupeMode = $('#AddDialog_DupeForce').is(':checked') ? "FORCE" : "SCORE";
			var params = info.password === '' ? [] : [{'*Unpack:Password' : info.password}];
			if (!$('#AddDialog_Fleet').is(':checked'))
			{
				// the box off: not grouped with copies of its release by the daemon either
				params.push({'DupeJoin': 'no'});
			}
			RPC.call('append', [
				filename,
				base64str,
				category,
				priority,
				false,
				addPaused,
				info.dupekey,
				info.dupescore,
				dupeMode,
				autoCategory,
				params,
			], fileCompleted, fileFailure);
		};

		reader.readAsDataURL(file);
	}

	function fileCompleted(result)
	{
		var failure = result < 0 || (result == 0 && Options.option('ScanScript') === '');
		errors |= failure;
		needRefresh |= !failure;
		if (!failure)
		{
			filesSuccess.push(files[index]);
			$('#AddDialog_Files table:eq(' + index + ') i.material-icon--success').toggleClass('hide');
		}
		else
		{
			$('#AddDialog_Files table:eq(' + index + ') i.material-icon--error').toggleClass('hide');
		}
		$('#AddDialog_Files table:eq(' + index + ') i.material-icon--progress').toggleClass('hide');
		index++;
		fileNext();
	}

	function fileFailure(res)
	{
		failure_message = res;
		// an RPC error: false read as 0, which counts as success when a scan
		// script is set, and the error was never shown
		fileCompleted(-1);
	}

	function urlNext()
	{
		$('#AddDialog_URLLabel img').show();
		$('#AddDialog_URLLabel i').hide();

		var name = urlInfo.name.toLowerCase();
		if (name !== '' && !(Util.endsWith(name, '.nzb') || Util.endsWith(name, '.zip') ||
			Util.endsWith(name, '.7z') || Util.endsWith(name, '.rar') || Util.endsWith(name, '.tar.gz')))
		{
			urlInfo.name += '.nzb';
		}
		
		var category = $('#AddDialog_Category').val();
		var priority = parseInt($('#AddDialog_Priority').val());
		var autoCategory = $('#AddDialog_AutoCategory').is(':checked');
		var addPaused = $('#AddDialog_Paused').is(':checked');
		var dupeMode = $('#AddDialog_DupeForce').is(':checked') ? "FORCE" : "SCORE";
		var params = urlInfo.password === '' ? [] : [{'*Unpack:Password' : urlInfo.password}];
		RPC.call('append', [
			urlInfo.name,
			url,
			category,
			priority,
			false,
			addPaused,
			urlInfo.dupekey,
			urlInfo.dupescore,
			dupeMode,
			autoCategory,
			params,
		], urlCompleted, urlFailure);
	}

	function urlCompleted(result)
	{
		var failure = result <= 0;
		errors |= failure;
		needRefresh |= !failure;
		if (result)
		{
			$('#AddDialog_URL').empty();
		}
		$('#AddDialog_URLLabel img').hide();
		$('#AddDialog_URLLabel i').removeClass('icon-ok').removeClass('icon-remove').addClass(
			result ? 'icon-ok' : 'icon-remove').show();

		filesStart();
	}

	function urlFailure(res)
	{
		failure_message = res;
		urlCompleted(false);
	}

	function allCompleted()
	{
		if (errors)
		{
			enableAllButtons();
			// using timeout for browser to update UI (buttons) before showing the alert
			setTimeout(function()
			{
				if (failure_message)
				{
					alert(I18n.translate(index > 1 ? 'msg_files_add_error' : 'msg_file_add_error_short', failure_message));
				}
				else
				{
					alert(I18n.translate(index > 1 ? 'msg_files_add_error_generic' : 'msg_file_add_error'));
				}
				needRefresh = true;
			}, 100);
		}
		else
		{
			$AddDialog.modal('hide');
			if (droppedCount > 0)
			{
				// exact copies of another file were not sent: say so
				PopupNotification.show('#Notif_AddFilesDropped');
			}
			else if (index > 0)
			{
				PopupNotification.show('#Notif_AddFiles');
			}
		}
	}

	function scan()
	{
		disableAllButtons();

		setTimeout(function(){
			$('#AddDialog_Transmit').show();
		}, 500);

		RPC.call('scan', [true], function()
		{
			needRefresh = true;
			$AddDialog.modal('hide');
			PopupNotification.show('#Notif_Scan');
		});
	}
}(jQuery));

/*** ADD FILE PARAM DIALOG *******************************************************/

var AddParamDialog = (new function($)
{
	'use strict'

	// Controls
	var $AddParamDialog;
	var $AddParamDialog_NZBName;
	var $AddParamDialog_Password;
	var $AddParamDialog_DupeKey;
	var $AddParamDialog_DupeScore;
	
	// State
	var info;
	var saveCallback;

	this.init = function()
	{
		$AddParamDialog = $('#AddParamDialog');
		$AddParamDialog_NZBName = $('#AddParamDialog_NZBName');
		$AddParamDialog_Password = $('#AddParamDialog_Password');
		$AddParamDialog_DupeKey = $('#AddParamDialog_DupeKey');
		$AddParamDialog_DupeScore = $('#AddParamDialog_DupeScore');
		
		$('#AddParamDialog_Save').click(save);

		$AddParamDialog.on('shown', function()
		{
			$('.modal-backdrop').last().addClass('modal-2');
			if (UISettings.setFocus)
			{
				$('#AddParamDialog_NZBName').focus();
			}
		});

		$AddParamDialog.on('hidden', function()
		{
			$('.modal-backdrop').removeClass('modal-2');
		});
	}

	this.showModal = function(_info, _saveCallback)
	{
		info = _info;
		saveCallback = _saveCallback;

		$AddParamDialog_NZBName.val(info.name);
		$AddParamDialog_Password.val(info.password);
		$AddParamDialog_DupeKey.val(info.dupekey);
		$AddParamDialog_DupeScore.val(info.dupescore);

		$AddParamDialog.modal({backdrop: 'static'});
	}

	function save(e)
	{
		info.name = $AddParamDialog_NZBName.val();
		info.password = $AddParamDialog_Password.val();
		info.dupekey = $AddParamDialog_DupeKey.val();
		info.dupescore = parseInt($AddParamDialog_DupeScore.val());

		$AddParamDialog.modal('hide');
		saveCallback(info);
	}
}(jQuery));
