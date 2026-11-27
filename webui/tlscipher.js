/* Rust TLS cipher optimizer dialog; provider credentials never enter this module. */
var TlsCipherDialog = (new function($)
{
	'use strict';
	var initialized = false, pending = false, timer = null, visible = false;
	var job = '', snapshot = null, appliedJob = '', state = null;
	var saveBlocked = false, needsStatus = false;
	// Apply now: 0 idle, 1 asking to confirm, 2 reloading. A retried server's job
	// reports only that server, so its row is merged into the earlier results.
	var applyStep = 0, retryJob = '', retryBase = null;
	function text(key, fallback) { return I18n.defaultValue('tls_cipher_' + key, fallback); }
	function active(result) { return ['measuring', 'probing', 'saving'].indexOf(result.State) !== -1; }
	function stopTimer() { if (timer !== null) clearTimeout(timer); timer = null; }
	function message(value) { $('#TlsCipherState').text(value); }
	function failure(value, result)
	{
		pending = false; stopTimer();
		needsStatus = true;
		message(result && result.error ? result.error.message : String(value));
		$('#TlsCipherCancel').prop('disabled', true);
		$('#TlsCipherAgain').show();
		$('#TlsCipherServers .tls-cipher-retry').prop('disabled', false);
	}
	function call(method, params, done)
	{
		RPC.call(method, params, done, failure, { timeout: 15000 });
	}
	function speed(value)
	{
		return typeof value === 'number' && isFinite(value) && value > 0 ?
			(value / 1000000).toFixed(1) + ' MB/s' : text('unmeasured', 'Unmeasured');
	}
	function cell(row, value) { $('<td>').text(value || '').appendTo(row); }
	// A server without a chosen cipher whose probes were inconclusive (DNS,
	// deadline, login) can be tested again; a fully tested server cannot.
	function retryable(item)
	{
		if (item.SelectedCipher || item.Outcome === 'saved' || !Number.isInteger(item.ID) || item.ID <= 0) return false;
		return !Array.isArray(item.CipherSupport) || item.CipherSupport.some(function(value) { return value !== true && value !== false; });
	}
	function mergedServers(result)
	{
		if (!retryJob || result.JobID !== retryJob || !retryBase || !retryBase.length) return result.Servers || [];
		var fresh = {}, seen = {};
		(result.Servers || []).forEach(function(item) { fresh[item.ID] = item; });
		var list = retryBase.map(function(item) {
			seen[item.ID] = true;
			return fresh.hasOwnProperty(item.ID) ? fresh[item.ID] : item;
		});
		return list.concat((result.Servers || []).filter(function(item) { return !seen[item.ID]; }));
	}
	function render(result)
	{
		state = result; job = result.JobID || '';
		var merged = retryJob !== '' && job === retryJob && retryBase !== null && retryBase.length > 0;
		message(result.Message || text(result.State, result.State));
		var progress = '';
		if (result.State === 'measuring' && result.CurrentAlgorithm)
			progress = text('testing_algorithm', 'Testing algorithm') + ': ' + result.CurrentAlgorithm;
		else if (result.State === 'probing')
			progress = (result.CompletedProbes || 0) + ' / ' + (result.TotalProbes || 0) + ' — ' +
				text('server', 'Server') + ' ' + (result.CurrentServer || '') + ': ' + (result.CurrentCipher || '');
		$('#TlsCipherProgress').text(progress).toggle(progress !== '');
		$('#TlsCipherExcluded').toggle((result.ExcludedSuites || []).length > 0)
			.find('p').text((result.ExcludedSuites || []).join(', '));
		$('#TlsCipherCancel').prop('disabled', !active(result));
		$('#TlsCipherAgain').toggle(!active(result) && result.Available !== false && (result.State !== 'idle' || needsStatus));
		var canApply = !active(result) && result.Saved === true && result.ReloadRequired === true;
		if (applyStep === 0 || !canApply)
		{
			if (applyStep !== 2) applyStep = 0;
			$('#TlsCipherReload').text(text('reload', 'Cipher settings are saved. Apply now reloads RustyGet so new connections use them.'))
				.toggle(canApply);
		}
		$('#TlsCipherApply').toggle(canApply && applyStep !== 2).prop('disabled', applyStep === 2)
			.text(applyStep === 1 ? text('apply_confirm', 'Reload now') : text('apply', 'Apply now'));
		var algorithms = $('#TlsCipherAlgorithms tbody').empty();
		(result.Algorithms || []).forEach(function(item) {
			var row = $('<tr>').appendTo(algorithms);
			cell(row, item.Algorithm); cell(row, speed(item.BytesPerSecond));
			cell(row, typeof item.BytesPerSecond === 'number' && item.BytesPerSecond > 0 && item.Minimum && item.Maximum ?
				speed(item.Minimum) + ' – ' + speed(item.Maximum) : item.Message);
		});
		var servers = $('#TlsCipherServers tbody').empty();
		var list = mergedServers(result);
		list.forEach(function(item) {
			var row = $('<tr>').appendTo(servers);
			cell(row, item.Name || 'Server ' + item.ID);
			cell(row, item.OldCipher || text('default', 'System default'));
			cell(row, item.SelectedCipher || '—');
			cell(row, item.Message || text(item.Outcome, item.Outcome || ''));
			if (!active(result) && applyStep !== 2 && retryable(item))
			{
				$('<button type="button" class="btn btn-small tls-cipher-retry">').text(text('retry', 'Retry'))
					.attr('data-server', item.ID).attr('aria-label', text('retry', 'Retry') + ': ' + (item.Name || 'Server ' + item.ID))
					.appendTo(row.children().last());
			}
			if (Array.isArray(item.CipherSupport) && Array.isArray(result.CipherSuites))
			{
				var details = $('<details>').appendTo(row.children().last());
				$('<summary>').text(text('compatibility', 'Cipher compatibility')).appendTo(details);
				var list = $('<ul>').appendTo(details);
				result.CipherSuites.forEach(function(cipher, index) {
					var support = item.CipherSupport[index];
					$('<li>').text(cipher + ': ' + (support === true ? text('supported', 'Supported') :
						support === false ? text('unsupported', 'Unsupported') : text('unknown', 'Unknown / untested'))).appendTo(list);
				});
			}
		});
		if (merged && !active(result)) { retryBase = list; }
		$('#TlsCipherCommon').text(merged || !Array.isArray(result.CommonSupported) ?
			text('common_unknown', 'Common supported ciphers: not fully established') :
			text('common', 'Common supported ciphers') + ': ' +
			((result.CommonSupported || []).join(', ') || text('none', 'None')));
		if (result.Saved === true && result.State === 'completed' && appliedJob !== job)
		{
			if (!Config.applyCipherResults(result, snapshot))
			{
				saveBlocked = true;
				message(text('browser_conflict', 'Settings were saved, but your server editor changed. Refresh Settings to see the saved ciphers; your edits have been retained.'));
			}
			else { appliedJob = job; saveBlocked = false; }
		}
		else if (active(result)) saveBlocked = true;
		else if (result.Saved === false) saveBlocked = false;
		needsStatus = false;
		stopTimer();
		if (active(result)) timer = setTimeout(poll, 500);
	}
	function poll()
	{
		stopTimer();
		if (pending) return;
		pending = true;
		call('tlscipherstatus', job && !needsStatus ? [job] : [], function(result) { pending = false; render(result); });
	}
	function start()
	{
		if (pending) return;
		snapshot = Config.cipherSnapshot();
		if (snapshot === null)
		{
			message(text('save_servers', 'Save your server changes before optimizing TLS ciphers.'));
			$('#TlsCipherCancel').prop('disabled', true); return;
		}
		pending = true; appliedJob = ''; saveBlocked = true; needsStatus = false;
		retryJob = ''; retryBase = null; applyStep = 0;
		$('#TlsCipherAgain').hide();
		message(text('starting', 'Starting cipher tests…'));
		call('tlscipherstart', [], function(result) { pending = false; render(result); });
	}
	function retry(server)
	{
		if (pending || (state && active(state)) || applyStep === 2) return;
		snapshot = Config.cipherSnapshot();
		if (snapshot === null)
		{
			message(text('save_servers', 'Save your server changes before optimizing TLS ciphers.'));
			return;
		}
		var base = state ? mergedServers(state) : [];
		pending = true; appliedJob = ''; saveBlocked = true; needsStatus = false; applyStep = 0;
		$('#TlsCipherServers .tls-cipher-retry').prop('disabled', true);
		message(text('retrying', 'Retrying this server…'));
		call('tlscipherstart', [String(server)], function(result) {
			pending = false;
			retryJob = result.JobID || ''; retryBase = base;
			render(result);
		});
	}
	// Reload RustyGet so new connections use the saved ciphers. The daemon reads
	// server settings when it starts; a reload waits for an unpack in progress.
	function apply()
	{
		if (applyStep === 0)
		{
			applyStep = 1;
			$('#TlsCipherReload').text(text('apply_warning', 'RustyGet reloads to use the saved ciphers: downloads pause for a few seconds. If an unpack is running, the reload waits until it finishes. Select Reload now to continue.')).show();
			$('#TlsCipherApply').text(text('apply_confirm', 'Reload now')).focus();
			return;
		}
		if (applyStep !== 1 || pending) return;
		applyStep = 2;
		$('#TlsCipherApply').hide();
		$('#TlsCipherAgain').hide();
		$('#TlsCipherServers .tls-cipher-retry').remove();
		var status = typeof Status !== 'undefined' && Status.status ? Status.status : {};
		var before = status.UpTimeSec, instance = status.Instance;
		var started = new Date().getTime();
		var info = $('#TlsCipherReload').text(text('applying', 'Reloading RustyGet to apply the saved ciphers…')).show();
		if (typeof Refresher !== 'undefined') Refresher.pause();
		var finish = function(key, fallback)
		{
			applyStep = 0;
			info.text(text(key, fallback));
			if (typeof Refresher !== 'undefined') Refresher.resume(true);
		};
		var check = function()
		{
			RPC.call('status', [], function(current) {
				var reloaded = instance !== undefined && current.Instance !== undefined ? current.Instance !== instance :
					before !== undefined && current.UpTimeSec < before;
				var waited = new Date().getTime() - started;
				if (reloaded)
				{
					if (typeof Status !== 'undefined') Status.status = current;
					state.ReloadRequired = false;
					$('#TlsCipherApply').hide();
					finish('applied', 'Applied: RustyGet reloaded and new connections use the saved ciphers.');
					return;
				}
				if (current.PostJobCount > 0 && waited < 6 * 3600 * 1000)
					info.text(text('apply_unpack', 'Waiting for the current unpack to finish; the ciphers apply when RustyGet reloads after it.'));
				else if (waited > 60000)
				{
					finish('apply_slow', 'RustyGet has not reloaded yet. It may still be finishing an unpack; check Messages or reopen this dialog later.');
					return;
				}
				setTimeout(check, 1000);
			}, function() {
				if (new Date().getTime() - started > 60000)
					finish('apply_failed', 'RustyGet did not answer after the reload request. Check that it is running.');
				else setTimeout(check, 1000);
			}, { timeout: 5000 });
		};
		RPC.call('reload', [], function() { setTimeout(check, 1000); }, function() { setTimeout(check, 1000); }, { timeout: 15000 });
	}
	this.init = function()
	{
		if (initialized) return;
		initialized = true;
		$('#TlsCipherDialog').on('hidden', function() {
			visible = false;
			// an unconfirmed Apply is dropped; a reload already requested keeps going
			if (applyStep === 1) applyStep = 0;
		})
			.on('shown', function() { $('#TlsCipherClose').focus(); });
		$('#TlsCipherCancel').on('click', function() {
			if (!job || !state || !active(state) || pending) return;
			pending = true; $('#TlsCipherCancel').prop('disabled', true);
			call('tlsciphercancel', [job], function(result) { pending = false; render(result); });
		});
		$('#TlsCipherAgain').on('click', function() { if (needsStatus) poll(); else start(); });
		$('#TlsCipherApply').on('click', apply);
		$('#TlsCipherServers').on('click', '.tls-cipher-retry', function() { retry(parseInt($(this).attr('data-server'), 10)); });
		$(window).on('beforeunload.tlscipher', stopTimer);
	};
	this.canSaveSettings = function() { return !saveBlocked; };
	this.showSaveBlocked = function()
	{
		$('#Notif_Config_CipherPending strong').text(text('save_pending',
			'Wait for TLS cipher testing to finish before saving Settings. If the server editor changed, refresh Settings to reconcile the saved ciphers.'));
		PopupNotification.show('#Notif_Config_CipherPending');
	};
	this.refreshAvailability = function()
	{
		var button = $('#Config_OptimizeTls');
		if (!button.length) return;
		button.prop('disabled', true);
		RPC.call('tlscipherstatus', [], function(result) {
			button.prop('disabled', result.Available !== true).attr('title', result.Available === true ? '' :
				(result.Message || text('unavailable', 'TLS cipher optimization requires a TLS-enabled Rust backend.')));
		}, function() {
			button.prop('disabled', true).attr('title', text('unavailable', 'TLS cipher optimization requires a TLS-enabled Rust backend.'));
		}, { timeout: 15000 });
	};
	this.showModal = function()
	{
		this.init(); visible = true;
		$('#TlsCipherDialog').modal({ backdrop: 'static' });
		if (pending) return;
		if (!snapshot) snapshot = Config.cipherSnapshot();
		pending = true;
		call('tlscipherstatus', [], function(result) {
			pending = false;
			if (result.Available === false)
			{
				render(result);
				message(result.Message || text('unavailable', 'TLS cipher optimization requires a TLS-enabled Rust backend.'));
			}
			else if (result.State === 'idle' && !needsStatus) start();
			else render(result);
		});
	};
}(jQuery));
