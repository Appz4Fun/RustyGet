/*
 * This file is part of RustyGet.
 *
 * This program is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 */

/*
 * First-run setup: the banner that says what still keeps RustyGet from
 * downloading (and what was wrong in the configuration file), and the setup
 * guide: (1) a password, (2) the download folders, checked as you type,
 * (3) a news server with a connection test, (4) save and restart.
 *
 * The daemon answers "setupstatus", "checkpath" and "testserver"; without
 * them (an older daemon, an offline fixture) nothing here shows.
 */
var Setup = (new function($)
{
	'use strict';

	var status = null;
	var config = null; // [{Name, Value}] from loadconfig
	var $Banner, $Wizard;
	var step = 1;
	var lastFocus = null;
	var checks = {}; // folder field -> {value, state, message, path}
	var checkTimers = {};
	var tested = null; // the server values that tested OK
	var testing = false;
	var SKIP_KEY = 'RustyGetSetupSkipped';
	var HIDE_NOTES_KEY = 'RustyGetConfigNotesHidden';
	var HIDE_PASSWORD_KEY = 'RustyGetPasswordHintHidden';

	function t(key, text) { return (typeof I18n !== 'undefined' && I18n.defaultValue) ? I18n.defaultValue(key, text) : text; }
	function esc(text) { return Util.textToHtml(String(text == null ? '' : text)); }
	function store(kind, key, value)
	{
		try
		{
			var s = kind === 'session' ? window.sessionStorage : window.localStorage;
			if (value === undefined) return s.getItem(key);
			if (value === null) s.removeItem(key); else s.setItem(key, value);
		}
		catch (e) { /* storage blocked: nothing remembered */ }
		return null;
	}

	this.init = function()
	{
		$Banner = $('<section id="SetupBanner" class="setup-banner" aria-live="polite" hidden></section>');
		$('#MainContent').prepend($Banner);
		// the list of things to do stays open on wide screens and folds on phones
		var wide = window.innerWidth > 600;
		$(window).on('resize', function()
		{
			var nowWide = window.innerWidth > 600;
			if (nowWide !== wide)
			{
				wide = nowWide;
				$Banner.find('.setup-banner__todo').prop('open', wide);
			}
		});
		$Banner.on('click', '[data-setup-action]', function(e)
		{
			e.preventDefault();
			action($(this).attr('data-setup-action'), $(this).attr('data-option'));
		});
	};

	// The setup state, or null when the daemon does not tell.
	this.status = function() { return status; };

	// Downloads stay paused until the password, folders and a news server are set.
	// Only a missing news server or unusable folders block downloading; a weak
	// password is a hint, never a reason to refuse Resume.
	this.blocked = function() { return !!status && status.DownloadsBlocked === true; };

	this.refresh = function(done)
	{
		RPC.call('setupstatus', [], function(result)
		{
			status = result && typeof result === 'object' ? result : null;
			render();
			maybeOpen();
			if (done) done();
		}, function()
		{
			status = null;
			render();
			if (done) done();
		});
	};

	// Resume was pressed while setup is not done: say what is missing.
	this.explainBlocked = function()
	{
		var todo = todoItems();
		var text = '<p>' + esc(t('setup_blocked_text', 'RustyGet can\'t download yet. Finish these first:')) + '</p><ul>';
		todo.forEach(function(item) { text += '<li>' + esc(item.text) + '</li>'; });
		text += '</ul>';
		AlertDialog.showModal(t('setup_blocked_title', 'Downloads are paused'), text);
	};

	function todoItems()
	{
		var items = [];
		if (!status) return items;
		(status.PathProblems || []).forEach(function(p)
		{
			items.push({ text: p.Option + ' (' + p.Value + '): ' + p.Problem,
				action: 'paths', option: p.Option, label: t('setup_btn_fix_folder', 'Choose a folder') });
		});
		if (!status.Servers)
		{
			items.push({ text: t('setup_todo_server', 'Add a news server and test the connection.'),
				action: 'server', label: t('setup_btn_server', 'Add news server') });
		}
		if (status.ConfigProblem)
		{
			items.push({ text: status.ConfigProblem, action: null });
		}
		return items;
	}

	function render()
	{
		if (!$Banner) return;
		if (!status)
		{
			$Banner.prop('hidden', true).empty();
			return;
		}
		var html = '';
		var todo = todoItems();
		var blocked = status.DownloadsBlocked === true;
		if (blocked || status.ConfigProblem)
		{
			html += '<div class="setup-banner__box setup-banner__box--todo" role="region" aria-labelledby="SetupBannerTitle">' +
				'<h3 id="SetupBannerTitle" class="setup-banner__title">' +
				(!blocked ? esc(t('setup_banner_title_save', 'Settings can\'t be saved')) :
					'<span class="setup-wide">' + esc(t('setup_banner_title', 'Downloads are paused until RustyGet is set up')) + '</span>' +
					'<span class="setup-narrow">' + esc(t('setup_banner_title_short', 'Downloads paused: setup not done')) + '</span>') + '</h3>' +
				// a phone shows the count and the guide button; the list opens on a tap
				'<details class="setup-banner__todo"' + (window.innerWidth > 600 ? ' open' : '') + '>' +
				'<summary>' + esc(todo.length === 1 ? t('setup_todo_one', '1 thing to do') :
					t('setup_todo_n', '%1 things to do').replace('%1', todo.length)) + '</summary><ul class="setup-banner__list">';
			todo.forEach(function(item)
			{
				html += '<li><span>' + esc(item.text) + '</span>' + (item.action ?
					' <button type="button" class="btn btn-small setup-banner__fix" data-setup-action="' + item.action + '"' +
					(item.option ? ' data-option="' + esc(item.option) + '"' : '') + '>' + esc(item.label) + '</button>' : '') + '</li>';
			});
			html += '</ul></details>';
			if (blocked)
			{
				html += '<button type="button" class="btn btn-primary" id="SetupBannerOpen" data-setup-action="open">' +
					esc(t('setup_btn_open', 'Open the setup guide')) + '</button>';
			}
			html += '</div>';
		}
		// a weak password: a gentle hint (it blocks nothing)
		if (status.PasswordSet === false && store('session', HIDE_PASSWORD_KEY) !== '1')
		{
			html += '<div class="setup-banner__box setup-banner__box--hint" role="region" aria-labelledby="SetupHintTitle">' +
				'<h3 id="SetupHintTitle" class="setup-banner__title">' + esc(status.PasswordIsStock ?
					t('setup_hint_stock', 'Tip: the web interface uses the stock password, which anyone can look up.') :
					t('setup_hint_none', 'Tip: the web interface has no password.')) + '</h3>' +
				'<p>' + esc(t('setup_hint_text', 'Set your own so that only you can control RustyGet. Downloading works either way.')) + '</p>' +
				'<button type="button" class="btn btn-small" data-setup-action="password">' + esc(t('setup_btn_password', 'Set a password')) + '</button> ' +
				'<button type="button" class="btn btn-small" data-setup-action="hidepassword">' + esc(t('setup_btn_later', 'Later')) + '</button>' +
				'</div>';
		}
		// in line order ("file:12: ..."), the file-wide ones first
		var lineOf = function(w) { var m = /:(\d+): /.exec(w); return m ? +m[1] : 0; };
		var warnings = (status.Warnings || []).slice().sort(function(a, b) { return lineOf(a) - lineOf(b); });
		if (warnings.length && store('session', HIDE_NOTES_KEY) !== status.ConfigFile)
		{
			var more = status.MoreWarnings || 0;
			var count = warnings.length + more;
			html += '<div class="setup-banner__box setup-banner__box--notes" role="region" aria-labelledby="SetupNotesTitle">' +
				'<h3 id="SetupNotesTitle" class="setup-banner__title">' +
				esc((count === 1 ? t('setup_notes_one', 'The configuration file had 1 problem.') :
					t('setup_notes_n', 'The configuration file had %1 problems.').replace('%1', count))) + '</h3>' +
				'<p>' + esc(t('setup_notes_text', 'RustyGet started anyway and used its defaults for those options. Check them in Settings; saving there writes a clean file.')) + '</p>' +
				'<details class="setup-banner__details"><summary>' + esc(t('setup_notes_show', 'Show the problems')) + '</summary><ul>';
			var file = status.ConfigFile || '';
			html += '<p class="setup-banner__file">' + esc(t('setup_notes_file', 'File: ')) + '<code>' + esc(file) + '</code></p>';
			warnings.forEach(function(w)
			{
				// "/path/rustyget.conf:12: text" -> "Line 12: text"
				var shown = file && w.indexOf(file + ':') === 0 ? w.substring(file.length + 1) : w;
				shown = shown.replace(/^(\d+): /, t('setup_notes_line', 'Line') + ' $1: ').replace(/^ /, '');
				html += '<li><code>' + esc(shown) + '</code></li>';
			});
			if (more) html += '<li>' + esc(t('setup_notes_more', '…and %1 more.').replace('%1', more)) + '</li>';
			html += '</ul></details>' +
				'<button type="button" class="btn btn-small" data-setup-action="settings">' + esc(t('setup_btn_settings', 'Open Settings')) + '</button> ' +
				'<button type="button" class="btn btn-small" data-setup-action="hidenotes">' + esc(t('setup_btn_hide', 'Hide')) + '</button>' +
				'</div>';
		}
		$Banner.html(html).prop('hidden', html === '');
	}

	function action(what, option)
	{
		switch (what)
		{
			case 'open': open(firstOpenStep()); break;
			case 'password':
				// a working setup only needs the password changed: Settings has it
				if (status && !status.DownloadsBlocked) { goSettings('SECURITY', 'ControlPassword'); }
				else { open(1); }
				break;
			case 'server': open(3); break;
			case 'paths':
				if (status && status.PasswordSet && status.Servers) { goSettings('PATHS', option); }
				else { open(2); }
				break;
			case 'settings': goSettings(); break;
			case 'hidepassword':
				store('session', HIDE_PASSWORD_KEY, '1');
				render();
				break;
			case 'hidenotes':
				store('session', HIDE_NOTES_KEY, status ? status.ConfigFile : '1');
				render();
				break;
		}
	}

	function goSettings(section, option)
	{
		$('#ConfigTabLink').tab('show');
		if (section)
		{
			setTimeout(function()
			{
				Config.navigateTo(section);
				if (option)
				{
					var control = $('#S_' + option);
					if (control.length) control.focus();
				}
			}, 400);
		}
	}

	function firstOpenStep()
	{
		if (!status) return 1;
		// the guide starts with the password: a new configuration shows the one
		// RustyGet made, and a missing one is asked for
		if (status.ConfigCreated || status.PasswordSet === false) return 1;
		if ((status.PathProblems || []).length) return 2;
		if (!status.Servers) return 3;
		return 1;
	}

	function maybeOpen()
	{
		if (!status || !status.DownloadsBlocked || !status.ConfigWritable) return;
		var skipped = store('local', SKIP_KEY) === (status.ConfigFile || '1');
		if (skipped) return;
		if (status.ConfigCreated || (status.PasswordSet === false && !status.Servers))
		{
			open(firstOpenStep());
		}
	}

	/*** THE GUIDE ***********************************************************/

	function value(name, def)
	{
		if (config)
		{
			for (var i = 0; i < config.length; i++)
			{
				if (config[i].Name.toLowerCase() === name.toLowerCase()) return config[i].Value;
			}
		}
		return def;
	}

	function open(atStep)
	{
		RPC.call('loadconfig', [], function(values)
		{
			config = values || [];
			build(atStep);
		}, function()
		{
			config = [];
			build(atStep);
		});
	}

	function build(atStep)
	{
		if ($Wizard) $Wizard.remove();
		lastFocus = document.activeElement;
		var current = value('ControlPassword', '');
		var passwordKept = current !== '' && current !== 'tegbzn6789';
		var html =
			'<div class="setup-overlay" id="SetupWizard">' +
			'<div class="setup-dialog" role="dialog" aria-modal="true" aria-labelledby="SetupTitle">' +
			'<div class="setup-head">' +
				'<h2 id="SetupTitle">' + esc(t('setup_title', 'Set up RustyGet')) + '</h2>' +
				'<ol class="setup-steps" aria-label="' + esc(t('setup_steps', 'Steps')) + '">' +
					'<li data-step="1">' + esc(t('setup_step_password', 'Password')) + '</li>' +
					'<li data-step="2">' + esc(t('setup_step_folders', 'Folders')) + '</li>' +
					'<li data-step="3">' + esc(t('setup_step_server', 'News server')) + '</li>' +
					'<li data-step="4">' + esc(t('setup_step_done', 'Done')) + '</li>' +
				'</ol>' +
			'</div>' +
			'<form class="setup-body" novalidate autocomplete="off">' +

			// 1 password
			'<fieldset class="setup-page" data-page="1"><legend>' + esc(t('setup_p1_title', '1. Protect the web interface')) + '</legend>' +
				(passwordKept ? '<p class="setup-current">' + esc(t('setup_p1_current', 'Current password: ')) +
					'<code id="SetupCurrentPass" data-secret="' + esc(current) + '">' + esc(current.replace(/./g, '\u2022')) + '</code> ' +
					'<button type="button" class="btn btn-small" id="SetupRevealPass" aria-controls="SetupCurrentPass" aria-pressed="false">' +
					esc(t('setup_p1_show', 'Show')) + '</button></p>' : '') +
				'<p class="setup-help">' + esc(passwordKept ?
					t('setup_p1_kept', 'This is the password you sign in with (RustyGet made it when it created the configuration file). Keep it by leaving the fields empty, or enter your own.') :
					t('setup_p1_help', 'Choose the user name and password you will sign in with. Anyone who can open this page without them could control RustyGet.')) + '</p>' +
				field('SetupUser', t('setup_user', 'User name'), 'text', value('ControlUsername', 'rustyget') || 'rustyget', 'username') +
				field('SetupPass', t('setup_pass', 'Password (at least 8 characters)'), 'password', '', 'new-password') +
				field('SetupPass2', t('setup_pass2', 'Password again'), 'password', '', 'new-password') +
				'<label class="setup-check"><input type="checkbox" id="SetupShowPass"> ' + esc(t('setup_show_pass', 'Show passwords')) + '</label>' +
			'</fieldset>' +

			// 2 folders
			'<fieldset class="setup-page" data-page="2"><legend>' + esc(t('setup_p2_title', '2. Choose where downloads go')) + '</legend>' +
				'<p class="setup-help">' + esc(t('setup_p2_help', 'RustyGet keeps its work files in the main folder and moves finished downloads to the second one. ~ means your home folder.')) + '</p>' +
				field('SetupMainDir', t('setup_maindir', 'Main folder (MainDir)'), 'text', value('MainDir', '~/RustyGet'), 'off') +
				field('SetupDestDir', t('setup_destdir', 'Finished downloads (DestDir)'), 'text', value('DestDir', '${MainDir}/dst'), 'off') +
				'<p class="setup-note">' + esc(t('setup_p2_note', 'Other folders (queue, temporary files, incoming NZB files) are created inside the main folder. You can change them later in Settings > Paths.')) + '</p>' +
			'</fieldset>' +

			// 3 news server
			'<fieldset class="setup-page" data-page="3"><legend>' + esc(t('setup_p3_title', '3. Add your news server')) + '</legend>' +
				'<p class="setup-help">' + esc(t('setup_p3_help', 'Your Usenet provider sent you these details when you signed up. Test the connection before you go on.')) + '</p>' +
				field('SetupHost', t('setup_host', 'Server address, for example news.example.com'), 'text', '', 'off') +
				'<div class="setup-row">' +
					field('SetupPort', t('setup_port', 'Port'), 'number', '563', 'off') +
					field('SetupConns', t('setup_conns', 'Connections'), 'number', '8', 'off') +
				'</div>' +
				'<label class="setup-check"><input type="checkbox" id="SetupTls" checked> ' + esc(t('setup_tls', 'Use an encrypted connection (SSL/TLS, recommended)')) + '</label>' +
				field('SetupSrvUser', t('setup_srv_user', 'Account user name'), 'text', '', 'off') +
				field('SetupSrvPass', t('setup_srv_pass', 'Account password'), 'password', '', 'off') +
				'<div class="setup-test"><button type="button" class="btn" id="SetupTest">' + esc(t('setup_test', 'Test connection')) + '</button>' +
				'<div class="setup-result" id="SetupTestResult" role="status" aria-live="polite"></div></div>' +
			'</fieldset>' +

			// 4 done
			'<fieldset class="setup-page" data-page="4"><legend>' + esc(t('setup_p4_title', '4. Save and start')) + '</legend>' +
				'<p class="setup-help">' + esc(t('setup_p4_help', 'Check the summary. Saving writes the settings (the old file is backed up first) and restarts RustyGet.')) + '</p>' +
				'<dl class="setup-summary" id="SetupSummary"></dl>' +
				'<div class="setup-result" id="SetupSaveResult" role="alert"></div>' +
			'</fieldset>' +
			'</form>' +
			'<div class="setup-foot">' +
				'<button type="button" class="btn-link setup-skip" id="SetupSkip">' + esc(t('setup_skip', 'Skip the guide (for experts): set things up in Settings')) + '</button>' +
				'<span class="setup-foot__buttons">' +
				'<button type="button" class="btn" id="SetupBack">' + esc(t('setup_back', 'Back')) + '</button>' +
				'<button type="button" class="btn btn-primary" id="SetupNext">' + esc(t('setup_next', 'Next')) + '</button>' +
				'</span>' +
				'<p class="setup-why" id="SetupWhy" aria-live="polite"></p>' +
			'</div>' +
			'</div></div>';
		$Wizard = $(html);
		$('body').append($Wizard);
		$('body').addClass('setup-open');
		tested = null;
		checks = {};

		$Wizard.on('input change', 'input', function() { onInput(this); });
		$Wizard.on('submit', 'form', function(e) { e.preventDefault(); next(); });
		// Enter in a field goes on (on the server page it tests first)
		$Wizard.on('keydown', 'input[type="text"], input[type="password"], input[type="number"]', function(e)
		{
			if (e.key !== 'Enter') return;
			e.preventDefault();
			if (step === 3 && (!tested || tested !== serverKey(serverValues()))) testServer();
			else next();
		});
		$('#SetupNext').on('click', next);
		$('#SetupBack').on('click', function() { show(step - 1); });
		$('#SetupSkip').on('click', skip);
		$('#SetupTest').on('click', testServer);
		$('#SetupRevealPass').on('click', function()
		{
			var code = $('#SetupCurrentPass');
			var shown = $(this).attr('aria-pressed') === 'true';
			code.text(shown ? code.attr('data-secret').replace(/./g, '\u2022') : code.attr('data-secret'));
			$(this).attr('aria-pressed', shown ? 'false' : 'true').text(shown ? t('setup_p1_show', 'Show') : t('setup_p1_hide', 'Hide'));
		});
		$('#SetupShowPass').on('change', function()
		{
			$('#SetupPass, #SetupPass2').attr('type', this.checked ? 'text' : 'password');
		});
		$('#SetupTls').on('change', function()
		{
			var port = $('#SetupPort');
			if (port.val() === '563' && !this.checked) port.val('119');
			else if (port.val() === '119' && this.checked) port.val('563');
			onInput(port[0]);
		});
		$Wizard.on('keydown', trapFocus);
		checkFolder('SetupMainDir');
		checkFolder('SetupDestDir');
		show(atStep || 1);
	}

	function field(id, label, type, val, autocomplete)
	{
		var extra = type === 'number' ? ' inputmode="numeric" min="1"' : '';
		return '<div class="setup-field" data-field="' + id + '">' +
			'<label for="' + id + '">' + esc(label) + '</label>' +
			'<input id="' + id + '" type="' + type + '" value="' + esc(val) + '" autocomplete="' + autocomplete + '"' +
			' spellcheck="false" aria-describedby="' + id + '_Msg"' + extra + '>' +
			'<div class="setup-msg" id="' + id + '_Msg"></div></div>';
	}

	function setMsg(id, kind, text)
	{
		var box = $('#' + id + '_Msg');
		box.removeClass('is-ok is-bad is-info').addClass(kind ? 'is-' + kind : '').text(text || '');
		$('#' + id).attr('aria-invalid', kind === 'bad' ? 'true' : 'false');
		$('#' + id).closest('.setup-field').toggleClass('has-problem', kind === 'bad');
	}

	function trapFocus(e)
	{
		if (e.key !== 'Tab') return;
		var items = $Wizard.find('button, input, a[href], summary').filter(':visible:not(:disabled)');
		if (!items.length) return;
		var first = items[0], last = items[items.length - 1];
		if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
		else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
	}

	function show(n)
	{
		step = Math.max(1, Math.min(4, n));
		$Wizard.find('.setup-page').each(function()
		{
			var on = +$(this).attr('data-page') === step;
			$(this).prop('hidden', !on);
		});
		$Wizard.find('.setup-steps li').each(function()
		{
			var s = +$(this).attr('data-step');
			$(this).toggleClass('is-done', s < step).toggleClass('is-current', s === step);
			if (s === step) $(this).attr('aria-current', 'step'); else $(this).removeAttr('aria-current');
		});
		$('#SetupBack').prop('hidden', step === 1);
		$('#SetupNext').text(step === 4 ? t('setup_save', 'Save and restart RustyGet') : t('setup_next', 'Next'));
		if (step === 4) summary();
		validateStep();
		var firstInput = $Wizard.find('.setup-page[data-page="' + step + '"] input:visible').first();
		(firstInput.length ? firstInput : $('#SetupNext')).focus();
	}

	function onInput(input)
	{
		var id = input.id;
		if (id === 'SetupMainDir' || id === 'SetupDestDir')
		{
			// leaving the field ("change") after the check: nothing new to check
			var known = checks[id];
			if (known && known.value === input.value && known.state !== 'checking')
			{
				validateStep();
				return;
			}
			clearTimeout(checkTimers[id]);
			setMsg(id, 'info', t('setup_checking', 'Checking…'));
			checkTimers[id] = setTimeout(function()
			{
				checkFolder(id);
				if (id === 'SetupMainDir') checkFolder('SetupDestDir');
			}, 350);
		}
		validateStep();
	}

	function checkFolder(id)
	{
		var val = $('#' + id).val();
		var mainDir = $('#SetupMainDir').val();
		checks[id] = { value: val, state: 'checking' };
		if (!$.trim(val))
		{
			checks[id] = { value: val, state: 'bad' };
			setMsg(id, 'bad', t('setup_folder_empty', 'Enter a folder.'));
			validateStep();
			return;
		}
		RPC.call('checkpath', [val, id === 'SetupMainDir' ? '' : mainDir], function(r)
		{
			if ($('#' + id).val() !== val) return; // typed on
			checks[id] = { value: val, state: r.State, path: r.Path, message: r.Message };
			setMsg(id, r.State === 'bad' ? 'bad' : 'ok', (r.State === 'bad' ? '' : r.Path + ': ') + r.Message);
			validateStep();
		}, function(message)
		{
			checks[id] = { value: val, state: 'bad' };
			setMsg(id, 'bad', t('setup_folder_nocheck', 'This folder could not be checked: ') + message);
			validateStep();
		});
	}

	function serverValues()
	{
		return {
			host: $.trim($('#SetupHost').val()),
			port: $.trim($('#SetupPort').val()),
			tls: $('#SetupTls').is(':checked'),
			user: $('#SetupSrvUser').val(),
			pass: $('#SetupSrvPass').val(),
			conns: $.trim($('#SetupConns').val())
		};
	}

	function serverKey(v) { return [v.host, v.port, v.tls, v.user, v.pass].join('\u0001'); }

	var HOST_RE = /^(\[[0-9a-fA-F:.%]+\]|[A-Za-z0-9_][A-Za-z0-9._-]*|[0-9a-fA-F:.%]+)$/;

	// Problems of the current page, in plain words; also marks the fields.
	function stepProblems(markAll)
	{
		var problems = [];
		function check(id, problem)
		{
			var touched = markAll || $('#' + id).data('touched') || $.trim($('#' + id).val()) !== '';
			if (problem) problems.push(problem);
			if (touched || !problem) setMsg(id, problem ? 'bad' : '', problem || '');
		}
		if (step === 1)
		{
			var user = $('#SetupUser').val(), p1 = $('#SetupPass').val(), p2 = $('#SetupPass2').val();
			var cur = value('ControlPassword', '');
			var keep = cur !== '' && cur !== 'tegbzn6789' && p1 === '' && p2 === '';
			check('SetupUser', user.indexOf(':') > -1 ? t('setup_user_colon', 'The user name can\'t contain ":".') :
				(user !== $.trim(user) ? t('setup_user_spaces', 'Remove the spaces at the start or end.') : ''));
			if (!keep)
			{
				check('SetupPass', p1.length < 8 ? t('setup_pass_short', 'Use at least 8 characters.') :
					(p1 !== $.trim(p1) ? t('setup_pass_spaces', 'Remove the spaces at the start or end.') : ''));
				check('SetupPass2', p2 !== p1 ? t('setup_pass_mismatch', 'The two passwords are not the same.') : '');
			}
		}
		else if (step === 2)
		{
			['SetupMainDir', 'SetupDestDir'].forEach(function(id)
			{
				var c = checks[id];
				if (!c || c.value !== $('#' + id).val() || c.state === 'checking')
				{
					problems.push(t('setup_wait_check', 'Wait for the folder check to finish.'));
				}
				else if (c.state === 'bad')
				{
					problems.push((id === 'SetupMainDir' ? 'MainDir' : 'DestDir') + ': ' + (c.message || t('setup_folder_bad', 'this folder can\'t be used.')));
				}
			});
		}
		else if (step === 3)
		{
			var v = serverValues();
			check('SetupHost', !v.host ? t('setup_host_empty', 'Enter the server address.') :
				(/^[a-z]+:\/\//i.test(v.host) ? t('setup_host_scheme', 'Enter only the name, without http:// or nntp://.') :
				(!HOST_RE.test(v.host) ? t('setup_host_bad', 'This is not a server address. Use letters, digits, dots and dashes, for example news.example.com.') : '')));
			var port = parseInt(v.port, 10);
			check('SetupPort', !/^[0-9]+$/.test(v.port) || port < 1 || port > 65535 ? t('setup_port_bad', 'Enter a port from 1 to 65535 (usually 563 with encryption, 119 without).') : '');
			var conns = parseInt(v.conns, 10);
			check('SetupConns', !/^[0-9]+$/.test(v.conns) || conns < 1 || conns > 999 ? t('setup_conns_bad', 'Enter a number from 1 to 999 (your provider says how many you may use).') : '');
			if (!problems.length && (!tested || tested !== serverKey(v)))
			{
				problems.push(t('setup_test_first', 'Test the connection first. Next is available once the test succeeds.'));
			}
		}
		return problems;
	}

	function validateStep()
	{
		if (!$Wizard) return true;
		var problems = stepProblems(false);
		var ok = problems.length === 0;
		$('#SetupNext').prop('disabled', !ok).attr('aria-describedby', ok ? null : 'SetupWhy');
		$('#SetupWhy').text(ok ? '' : problems[0]);
		return ok;
	}

	function next()
	{
		$Wizard.find('.setup-page[data-page="' + step + '"] input').data('touched', true);
		var problems = stepProblems(true);
		if (problems.length)
		{
			validateStep();
			var bad = $Wizard.find('.setup-page[data-page="' + step + '"] [aria-invalid="true"]').first();
			if (bad.length) bad.focus();
			return;
		}
		if (step < 4) show(step + 1);
		else save();
	}

	// "connection refused" and friends, in plain words, with what to do.
	function explainError(text)
	{
		var s = String(text).toLowerCase();
		if (/resolve|getaddrinfo|name or service|nodename|unknown host|host not found/.test(s))
			return t('setup_err_dns', 'The server address could not be found (DNS). Check the spelling of the address.');
		if (/tls|ssl|certificate|handshake|x509/.test(s))
			return t('setup_err_tls', 'The encrypted connection failed (TLS). Check that encryption is on only if the server supports it, and that the port is the encrypted one (often 563).');
		if (/auth|481|482|password|username|login|denied/.test(s))
			return t('setup_err_auth', 'The server refused the user name or password. Check the account details from your provider.');
		if (/refused|timed out|timeout|unreachable|could not connect|connection reset|no route/.test(s))
			return t('setup_err_port', 'The server did not answer on this port. Check the port (563 with encryption, 119 without) and your firewall.');
		return t('setup_err_other', 'The test failed. Check the details below and try again.');
	}

	function testServer()
	{
		if (testing) return;
		var v = serverValues();
		$Wizard.find('[data-page="3"] input').data('touched', true);
		var problems = stepProblems(true).filter(function(p) { return p.indexOf('Test the connection') !== 0; });
		if (problems.length && problems[0] !== t('setup_test_first', 'Test the connection first. Next is available once the test succeeds.'))
		{
			$('#SetupTestResult').removeClass('is-ok').addClass('is-bad').text(problems[0]);
			return;
		}
		testing = true;
		$('#SetupTest').prop('disabled', true).text(t('setup_testing', 'Testing…'));
		$('#SetupTestResult').removeClass('is-ok is-bad').text(t('setup_testing_long', 'Connecting to ') + v.host + ':' + v.port + '…');
		RPC.call('testserver', [v.host, parseInt(v.port, 10), v.user, v.pass, v.tls, '', 10, 2],
			function(errtext)
			{
				testing = false;
				$('#SetupTest').prop('disabled', false).text(t('setup_test', 'Test connection'));
				if (errtext === '' || errtext === null)
				{
					tested = serverKey(v);
					$('#SetupTestResult').removeClass('is-bad').addClass('is-ok')
						.text(t('setup_test_ok', 'Connected to %1 and signed in. You can go on.').replace('%1', v.host));
				}
				else
				{
					tested = null;
					$('#SetupTestResult').removeClass('is-ok').addClass('is-bad')
						.html('<strong>' + esc(explainError(errtext)) + '</strong><br><span class="setup-raw">' + esc(t('setup_test_detail', 'Server said: ')) + esc(errtext) + '</span>');
				}
				validateStep();
			},
			function(message, result)
			{
				testing = false;
				tested = null;
				$('#SetupTest').prop('disabled', false).text(t('setup_test', 'Test connection'));
				var text = result && result.error && result.error.message ? result.error.message : message;
				$('#SetupTestResult').removeClass('is-ok').addClass('is-bad')
					.html('<strong>' + esc(explainError(text)) + '</strong><br><span class="setup-raw">' + esc(text) + '</span>');
				validateStep();
			});
	}

	function summary()
	{
		var v = serverValues();
		var newPass = $('#SetupPass').val() !== '';
		var rows = [
			[t('setup_sum_user', 'Sign in as'), $('#SetupUser').val() + (newPass ? '' : ' ' + t('setup_sum_keep', '(password unchanged)'))],
			[t('setup_sum_main', 'Main folder'), (checks.SetupMainDir && checks.SetupMainDir.path) || $('#SetupMainDir').val()],
			[t('setup_sum_dest', 'Finished downloads'), (checks.SetupDestDir && checks.SetupDestDir.path) || $('#SetupDestDir').val()],
			[t('setup_sum_server', 'News server'), v.host + ':' + v.port + (v.tls ? ' (' + t('setup_sum_tls', 'encrypted') + ')' : '') + ', ' + v.conns + ' ' + t('setup_sum_conns', 'connections')]
		];
		$('#SetupSummary').html(rows.map(function(r) { return '<dt>' + esc(r[0]) + '</dt><dd>' + esc(r[1]) + '</dd>'; }).join(''));
	}

	function serverIndex()
	{
		var used = {}, max = 0;
		(config || []).forEach(function(o)
		{
			var m = /^Server([0-9]+)\.Host$/i.exec(o.Name);
			if (m)
			{
				var n = +m[1];
				max = Math.max(max, n);
				used[n] = $.trim(o.Value) !== '';
			}
		});
		for (var i = 1; i <= max; i++)
		{
			if (used[i] === false) return i;
		}
		return max + 1;
	}

	function save()
	{
		var v = serverValues();
		var n = serverIndex();
		var changes = {
			ControlUsername: $('#SetupUser').val(),
			MainDir: $('#SetupMainDir').val(),
			DestDir: $('#SetupDestDir').val()
		};
		if ($('#SetupPass').val() !== '') changes.ControlPassword = $('#SetupPass').val();
		var p = 'Server' + n + '.';
		changes[p + 'Active'] = 'yes';
		changes[p + 'Name'] = v.host;
		changes[p + 'Host'] = v.host;
		changes[p + 'Port'] = v.port;
		changes[p + 'Encryption'] = v.tls ? 'yes' : 'no';
		changes[p + 'Username'] = v.user;
		changes[p + 'Password'] = v.pass;
		changes[p + 'Connections'] = v.conns;
		var request = [], done = {};
		(config || []).forEach(function(o)
		{
			var key = Object.keys(changes).filter(function(k) { return k.toLowerCase() === o.Name.toLowerCase(); })[0];
			request.push({ Name: o.Name, Value: key ? changes[key] : o.Value });
			if (key) done[key] = true;
		});
		Object.keys(changes).forEach(function(k) { if (!done[k]) request.push({ Name: k, Value: changes[k] }); });

		$('#SetupNext').prop('disabled', true).text(t('setup_saving', 'Saving…'));
		$('#SetupSaveResult').removeClass('is-bad').text('');
		var changed = Object.keys(changes).map(function(k) { return k.toLowerCase(); });
		RPC.call('checkconfig', [request], function(problems)
		{
			problems = problems || [];
			var mine = problems.filter(function(p) { return changed.indexOf(String(p.Name).toLowerCase()) > -1; });
			if (mine.length)
			{
				saveFailed(mine.map(function(p) { return p.Problem; }).join(' '));
				return;
			}
			if (!problems.length)
			{
				send(request, changes);
				return;
			}
			confirmDrop(problems, function()
			{
				var drop = problems.map(function(p) { return String(p.Name).toLowerCase(); });
				send(request.filter(function(o) { return drop.indexOf(o.Name.toLowerCase()) === -1; }), changes);
			});
		}, function()
		{
			// an older daemon without checkconfig: save as it is
			send(request, changes);
		});
	}

	// The configuration file has lines that can't be kept: list them, and
	// remove them only when the user agrees.
	function confirmDrop(problems, proceed)
	{
		var box = $('#SetupSaveResult').removeClass('is-bad').addClass('setup-drop');
		box.html('<p><strong>' + esc(problems.length === 1 ?
			t('setup_drop_one', 'One setting in your configuration file can\'t be kept:') :
			t('setup_drop_n', '%1 settings in your configuration file can\'t be kept:').replace('%1', problems.length)) + '</strong></p><ul>' +
			problems.map(function(p) { return '<li>' + esc(p.Problem) + '</li>'; }).join('') + '</ul>' +
			'<p>' + esc(t('setup_drop_text', 'They will be removed from the file; RustyGet uses its defaults for them. A backup of the file is made first.')) + '</p>' +
			'<button type="button" class="btn btn-primary" id="SetupDropOk">' + esc(t('setup_drop_ok', 'Remove them and save')) + '</button> ' +
			'<button type="button" class="btn" id="SetupDropCancel">' + esc(t('setup_drop_cancel', 'Don\'t save')) + '</button>');
		$('#SetupNext').prop('disabled', true).text(t('setup_save', 'Save and restart RustyGet'));
		$('#SetupDropOk').on('click', function() { box.empty().removeClass('setup-drop'); proceed(); }).focus();
		$('#SetupDropCancel').on('click', function()
		{
			box.empty().removeClass('setup-drop');
			$('#SetupNext').prop('disabled', false);
			$('#SetupNext').focus();
		});
	}

	function send(request, changes)
	{
		$('#SetupNext').prop('disabled', true).text(t('setup_saving', 'Saving…'));
		RPC.call('saveconfig', [request], function(ok)
		{
			if (!ok)
			{
				saveFailed(t('setup_save_failed', 'The settings could not be saved. Check the Messages tab for the reason.'));
				return;
			}
			store('local', SKIP_KEY, null);
			restarting(changes.ControlPassword !== undefined);
		}, function(message, result)
		{
			saveFailed(result && result.error && result.error.message ? result.error.message : message);
		});
	}

	function saveFailed(text)
	{
		$('#SetupSaveResult').addClass('is-bad').text(text);
		$('#SetupNext').prop('disabled', false).text(t('setup_save', 'Save and restart RustyGet'));
		$('#SetupSaveResult').attr('tabindex', '-1').focus();
	}

	function restarting(newPassword)
	{
		$Wizard.find('.setup-body').html(
			'<div class="setup-restart" role="status" aria-live="polite">' +
			'<h3>' + esc(t('setup_restarting', 'Settings saved. RustyGet is restarting…')) + '</h3>' +
			(newPassword ? '<p>' + esc(t('setup_signin', 'When the page comes back, sign in with the user name and password you just chose.')) + '</p>' : '') +
			'</div>');
		$Wizard.find('.setup-foot').hide();
		var upBefore = (typeof Status !== 'undefined' && Status.status) ? Status.status.UpTimeSec : null;
		RPC.call('reload', [], function() {}, function() {});
		var started = Date.now();
		var sawDown = false;
		// the new instance is up when it asks for the new password, or reports a
		// shorter uptime than the old one did
		function poll()
		{
			var xhr = new XMLHttpRequest();
			xhr.open('get', UISettings.rpcUrl + '/status');
			xhr.onreadystatechange = function()
			{
				if (xhr.readyState !== 4) return;
				var restarted = xhr.status === 401 || (xhr.status === 200 && sawDown);
				if (xhr.status !== 200 && xhr.status !== 401) sawDown = true;
				if (xhr.status === 200)
				{
					try
					{
						var up = JSON.parse(xhr.responseText).result.UpTimeSec;
						restarted = restarted || (upBefore === null ? Date.now() - started > 4000 : up < upBefore);
					}
					catch (e) { restarted = false; }
				}
				if (restarted) document.location.reload();
				else setTimeout(poll, 600);
			};
			xhr.send();
		}
		setTimeout(poll, 800);
	}

	function skip()
	{
		store('local', SKIP_KEY, (status && status.ConfigFile) || '1');
		close();
	}

	function close()
	{
		if ($Wizard) $Wizard.remove();
		$Wizard = null;
		$('body').removeClass('setup-open');
		render();
		var target = $('#SetupBannerOpen');
		(target.length ? target : $(lastFocus)).focus();
	}
}(jQuery));
