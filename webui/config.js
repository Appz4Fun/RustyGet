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
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

/*
 * In this module:
 *   1) Loading of program options and post-processing scripts options;
 *   2) Settings tab;
 *   3) Function "Reload".
 */

/*** OPTIONS AND CONFIGS (FROM CONFIG FILES) **************************************/

function Server()
{
	this.id = 0;
	this.host = '';
	this.name = '';
	this.port = 0;
	this.connections = 0;
}

var Options = (new function($)
{
	'use strict';

	// Properties (public)
	this.options;
	this.postParamConfig = [];
	this.configtemplates = [];
	this.categories = [];
	this.restricted = false;

	// State
	var _this = this;
	this.serverTemplateData = null;
	var serverValues;
	var loadComplete;
	var loadConfigError;
	var loadServerTemplateError;
	var shortScriptNames = [];
	var subs = [];
	var onPageRenderedCallback;

	var HIDDEN_SECTIONS = ['DISPLAY (TERMINAL)', 'POSTPROCESSING-PARAMETERS', 'POST-PROCESSING-PARAMETERS', 'POST-PROCESSING PARAMETERS'];
	var POSTPARAM_SECTIONS = ['POSTPROCESSING-PARAMETERS', 'POST-PROCESSING-PARAMETERS', 'POST-PROCESSING PARAMETERS'];

	this.update = function()
	{
		// RPC-function "config" returns CURRENT configuration settings loaded in RustyGet
		RPC.call('config', [], function(_options)
		{
			_this.options = _options;
			initCategories();
			_this.restricted = _this.option('ControlPort') === '***';
			notifyAll(_options);
			RPC.next();
		});

		// load config templates
		RPC.call('configtemplates', [false], function(data)
		{
			_this.configtemplates = data;
			RPC.next();
		});

		// build list of post-processing parameters
		_this.postParamConfig = initPostParamConfig();
	}

	// re-read the running configuration (after a reload) for the pages that use it,
	// without touching the Settings editor
	this.refreshRuntime = function(done)
	{
		RPC.call('config', [], function(_options)
		{
			_this.options = _options;
			initCategories();
			_this.restricted = _this.option('ControlPort') === '***';
			notifyAll(_options);
			if (done) done();
		}, function() { if (done) done(); });
	}

	this.setOnPageRenderedCallback = function(callback) {
		onPageRenderedCallback = callback;
	}

	this.cleanup = function()
	{
		this.serverTemplateData = null;
		serverValues = null;
	}

	this.option = function(name)
	{
		var opt = findOption(this.options, name);
		return opt ? opt.Value : null;
	}

	this.getServerById = function(id)
	{
		var server = new Server();

		server.id = id;
		var serverId = 'Server' + id;
		var hostOpt = findOption(this.options, serverId + '.Host');
		if (!hostOpt)
			return null;

		server.host = hostOpt.Value;

		var nameOpt = findOption(this.options, serverId + '.Name');
		if (nameOpt)
			server.name = nameOpt.Value;
		
		var portOpt = findOption(this.options, serverId + '.Port');
		if (portOpt)
			server.port = portOpt.Value;

		var activeOpt = findOption(this.options, serverId + '.Active');
		if (activeOpt)
			server.active = activeOpt.Value == 'yes';

		var connectionsOpt = findOption(this.options, serverId + '.Connections');
		if (connectionsOpt)
			server.connections = connectionsOpt.Value;

		return server;
	}

	this.subscribe = function(sub)
	{
		subs.push(sub);
	}

	function notifyAll(options) {
		for (var i = 0; i < subs.length; ++i) {
			subs[i].update(options);
		}
	}

	function initCategories()
	{
		_this.categories = [];
		for (var i=0; i < _this.options.length; i++)
		{
			var option = _this.options[i];
			if ((option.Name.toLowerCase().substring(0, 8) === 'category') && (option.Name.toLowerCase().indexOf('.name') > -1))
			{
				_this.categories.push(option.Value);
			}
		}
	}

	/*** LOADING CONFIG ********************************************************************/

	this.loadConfig = function(callbacks)
	{
		loadComplete = callbacks.complete;
		loadConfigError = callbacks.configError;
		loadServerTemplateError = callbacks.serverTemplateError;

		// RPC-function "loadconfig" reads the configuration settings from RustyGet configuration file.
		// that's not neccessary the same settings returned by RPC-function "config". This could be the case,
		// for example, if the settings were modified but RustyGet was not restarted.
		RPC.call('loadconfig', [], serverValuesLoaded, loadConfigError);
	}

	this.complete = function() 
	{
		if (this.serverTemplateData === null) 
		{
			// the loading was cancelled and the data was discarded (via method "cleanup()")
			return;
		}

		var config = [];
		config.values = serverValues;

		readWebSettings(config);

		var serverConfig = readConfigTemplate(this.serverTemplateData[0].Template, undefined, HIDDEN_SECTIONS, '');
		mergeValues(serverConfig.sections, serverValues);
		config.push(serverConfig);

		serverValues = null;
		loadComplete(config);
		if (onPageRenderedCallback)
		{
			onPageRenderedCallback();
			onPageRenderedCallback = null;
		}
	}

	// ServerN.Level (0-99) became ServerN.Priority (1-5): level 0 is priority 1
	function upgradeValues(values)
	{
		if (!values)
		{
			return values;
		}
		var names = {}, raw = {}, dropped = [];
		values.forEach(function(v) { names[v.Name.toLowerCase()] = true; raw[v.Name.toLowerCase()] = v.Value; });
		// the daemon expands ${Option} references, so Server2.Level=${Server1.Level} is that level
		function expand(value, depth)
		{
			return String(value).replace(/\$\{([^}]+)\}/g, function(all, name)
			{
				var target = raw[name.toLowerCase()];
				return depth > 8 || target === undefined ? '' : expand(target, depth + 1);
			});
		}
		values.forEach(function(v)
		{
			// WriteLog (none, append, reset, rotate) became the LogToFile switch
			if (/^WriteLog$/i.test(v.Name) && !names.logtofile)
			{
				v.Name = 'LogToFile';
				v.Value = expand(v.Value, 0).trim().toLowerCase() === 'none' ? 'no' : 'yes';
			}
			// ServerN.Group (0-99) became ServerN.Backbone: a group above 0 is the backbone "Group N"
			// (an empty Backbone next to it still takes the group, as the daemon does)
			var g = /^Server([0-9]+)\.Group$/i.exec(v.Name);
			if (g)
			{
				var group = parseInt(expand(v.Value, 0), 10) || 0;
				var backbone = values.filter(function(o) { return o.Name.toLowerCase() === ('Server' + g[1] + '.Backbone').toLowerCase(); })[0];
				if (!backbone)
				{
					v.Name = 'Server' + g[1] + '.Backbone';
					v.Value = group > 0 ? 'Group ' + group : '';
				}
				else
				{
					if (!$.trim(expand(backbone.Value, 0)) && group > 0)
					{
						backbone.Value = 'Group ' + group;
					}
					dropped.push(v);
				}
			}
			var m = /^Server([0-9]+)\.Level$/i.exec(v.Name);
			if (m && !names[('Server' + m[1] + '.Priority').toLowerCase()])
			{
				var level = parseInt(expand(v.Value, 0), 10) || 0;
				v.Name = 'Server' + m[1] + '.Priority';
				v.Value = String(Math.max(0, Math.min(4, level)) + 1);
			}
		});
		dropped.forEach(function(v) { values.splice(values.indexOf(v), 1); });
		return values;
	}
	this.upgradeValues = upgradeValues;

	function serverValuesLoaded(data)
	{
		serverValues = upgradeValues(data);
		RPC.call('configtemplates', [true], serverTemplateLoaded, loadServerTemplateError);
	}

	function serverTemplateLoaded(data)
	{
		Options.serverTemplateData = data;
		Options.complete();
	}

	function readWebSettings(config)
	{
		var webTemplate = '### WEB-INTERFACE ###\n\n';
		var webValues = [];

		for (var optname in UISettings.description)
		{
			var descript = UISettings.description[optname];
			var value = UISettings[optname];
			optname = optname[0].toUpperCase() + optname.substring(1);
			if (value === true) value = 'yes';
			if (value === false) value = 'no';

			descript = descript.replace(/\n/g, '\n# ').replace(/\n# \n/g, '\n#\n');
			webTemplate += '# ' + descript + '\n' + optname + '=' + value + '\n\n';

			webValues.push({Name: optname, Value: value.toString()});
		}

		var webConfig = readConfigTemplate(webTemplate, undefined, '', '', 'webui_config_desc_');
		mergeValues(webConfig.sections, webValues);
		config.push(webConfig);
	}

	this.reloadConfig = function(_serverValues, _complete)
	{
		loadComplete = _complete;
		serverValues = upgradeValues(_serverValues);
		Options.complete();
	}

	/*** PARSE CONFIG AND BUILD INTERNAL STRUCTURES **********************************************/

	function readConfigTemplate(filedata, visiblesections, hiddensections, nameprefix, keyPrefix)
	{
		var config = { nameprefix: nameprefix, sections: [], };
		var section = null;
		var description = '';
		var firstdescrline = '';

		var data = filedata.split('\n');

		// Splits "Category2.Unpack" into family "category.unpack" and index "2".
		function multiOptionIndex(name)
		{
			var m = /^([^\s=@#]*?)([0-9]+)\.([^\s=@]+)$/.exec(name.trim());
			return m ? { family: (m[1] + '.' + m[3]).toLowerCase(), index: m[2] } : null;
		}

		// Multi-section options are declared with index "1"; other instances are appended at EOF.
		var multiOptionFamilies = {};
		for (var mi=0; mi < data.length; mi++)
		{
			var nameline = data[mi].replace(/\r+$/,'').trim().replace(/^#\s*/, '');
			var eqpos = nameline.search(/[=@]/);
			if (eqpos > 0)
			{
				var m = multiOptionIndex(nameline.substring(0, eqpos));
				if (m && m.index === '1')
				{
					multiOptionFamilies[m.family] = true;
				}
			}
		}

		for (var i=0, len=data.length; i < len; i++)
		{
			var line = data[i].replace(/\r+$/,''); // remove possible trailing CR-characters

			if (line.substring(0, 4) === '### ')
			{
				var section = {};
				section.name = line.substr(4, line.length - 8).trim();
				section.id = Util.makeId(nameprefix + section.name);
				section.options = [];
				description = '';
				section.hidden = !(hiddensections === undefined || (hiddensections.indexOf(section.name) == -1)) ||
					(visiblesections !== undefined && (visiblesections.indexOf(section.name) == -1));
				section.postparam = POSTPARAM_SECTIONS.indexOf(section.name) > -1;
				config.sections.push(section);
			}
			else if (line.substring(0, 2) === '# ' || line === '#')
			{
				if (description !== '')
				{
					description += ' ';
				}
				if (line[2] === ' ' && line[3] !== ' ' && description.substring(description.length-4, 4) != '\n \n ')
				{
					description += '\n';
				}
				description += line.substr(1, 10000).trim();
				var lastchar = description.substr(description.length - 1, 1);
				if (lastchar === '.' && firstdescrline === '')
				{
					firstdescrline = description;
					description = '';
				}
				// Only add newline when line is '#' alone (empty line in config = paragraph break)
				if (line === '#')
				{
					description += '\n';
				}
			}
			else if (line.indexOf('=') > -1 || line.indexOf('@') > -1)
			{
				if (!section)
				{
					// bad template file; create default section.
					section = {};
					section.name = 'OPTIONS';
					section.id = Util.makeId(nameprefix + section.name);
					section.options = [];
					description = '';
					config.sections.push(section);
				}

				var option = {};
				var enabled = line.substr(0, 1) !== '#';
				var command = line.indexOf('=') === -1 && line.indexOf('@') > -1;
				option.caption = line.substr(enabled ? 0 : 1, line.indexOf(command ? '@' : '=') - (enabled ? 0 : 1)).trim();
				if (command)
				{
					var optpos = option.caption.indexOf('[');
					option.commandopts = optpos > -1 ? option.caption.substring(optpos + 1, option.caption.indexOf(']')).toLowerCase() : 'settings';
					option.caption = optpos > -1 ? option.caption.substring(0, optpos) : option.caption;
				}

				// A value line of a non-first instance is not a declaration: it would duplicate the option.
				var inst = multiOptionIndex(option.caption);
				if (inst && inst.index !== '1' && multiOptionFamilies[inst.family])
				{
					description = '';
					firstdescrline = '';
					continue;
				}

				option.name = (nameprefix != '' ? nameprefix : '') + option.caption;
				option.defvalue = line.substr(line.indexOf(command ? '@' : '=') + 1, 1000).trim();
				option.value = null;
				option.sectionId = section.id;
				option.select = [];
				option.keyPrefix = keyPrefix;

				var pstart = firstdescrline.lastIndexOf('(');
				var pend = firstdescrline.lastIndexOf(')');
				if (pstart > -1 && pend > -1 && pend === firstdescrline.length - 2)
				{
					var paramstr = firstdescrline.substr(pstart + 1, pend - pstart - 1);
					var params = paramstr.split(',');
					for (var pj=0; pj < params.length; pj++)
					{
						option.select.push(params[pj].trim());
					}
					firstdescrline = firstdescrline.substr(0, pstart).trim() + '.';
				}

				if (option.name.substr(nameprefix.length, 1000).indexOf('1.') > -1)
				{
					section.multi = true;
					section.multiprefix = option.name.substr(0, option.name.indexOf('1.'));
				}

				if (!section.multi || option.name.indexOf('1.') > -1)
				{
					section.options.push(option);
				}

				if (section.multi)
				{
					option.template = true;
				}

				option.description = firstdescrline + description;
				description = '';
				firstdescrline = '';
			}
			else
			{
				if (section && section.options.length === 0)
				{
					section.description = firstdescrline + description;
				}
				description = '';
				firstdescrline = '';
			}
		}

		return config;
	}

	function findOption(options, name)
	{
		if (!options)
		{
			return null;
		}

		name = name.toLowerCase();

		for (var i=0; i < options.length; i++)
		{
			var option = options[i];
			if ((option.Name && option.Name.toLowerCase() === name) ||
				(option.name && option.name.toLowerCase() === name))
			{
				return option;
			}
		}

		return null;
	}
	this.findOption = findOption;

	function mergeValues(config, values)
	{
		// copy values
		for (var i=0; i < config.length; i++)
		{
			var section = config[i];
			if (section.multi)
			{
				// multi sections (news-servers, scheduler)

				var subexists = true;
				for (var k=1; subexists; k++)
				{
					subexists = false;
					for (var m=0; m < section.options.length; m++)
					{
						var option = section.options[m];
						if (option.name.indexOf('1.') > -1)
						{
							var name = option.name.replace(/1/, k);
							var val = findOption(values, name);
							if (val)
							{
								subexists = true;
								break;
							}
						}
					}
					if (subexists)
					{
						for (var m=0; m < section.options.length; m++)
						{
							var option = section.options[m];
							if (option.template)
							{
								var name = option.name.replace(/1/, k);
								// copy option
								var newoption = $.extend({}, option);
								newoption.name = name;
								newoption.caption = option.caption.replace(/1/, k);
								newoption.template = false;
								newoption.multiid = k;
								newoption.value = null;
								section.options.push(newoption);
								var val = findOption(values, name);
								if (val)
								{
									newoption.value = val.Value;
								}
								else if (/^Server[0-9]+\.Active$/i.test(name))
								{
									// RustyGet uses a server without an Active line: show it
									// (and save it) as on, not as the template example's "no"
									newoption.value = 'yes';
								}
							}
						}
					}
				}
			}
			else
			{
				// simple sections

				for (var j=0; j < section.options.length; j++)
				{
					var option = section.options[j];
					option.value = null;
					var val = findOption(values, option.name);
					if (val && !option.commandopts)
					{
						option.value = val.Value;
					}
					else if (option.name === 'ParPreset' && values && values.length)
					{
						// a config written before ParPreset existed runs as custom
						// (its own ParBuffer, ParThreads, ParPauseQueue): show that,
						// and don't let a save switch it to the template's level
						option.value = 'custom';
					}
				}
			}
		}
	}
	this.mergeValues = mergeValues;

	function shortScriptName(scriptName)
	{
		var shortName = shortScriptNames[scriptName];
		return shortName ? shortName : scriptName;
	}
	this.shortScriptName = shortScriptName;

	function initPostParamConfig()
	{
		// One post-processing parameter section; the built-in parameters
		// (such as *Unpack:) are added to it from the configuration template.
		var section = {};
		section.id = 'PP-Parameters';
		section.options = [];
		section.description = '';
		section.about = '';
		section.hidden = false;
		section.postparam = true;
		return [section];
	}
}(jQuery));


/*** SETTINGS TAB (UI) *********************************************************/

var Config = (new function($)
{
	'use strict';

	// Controls
	var $ConfigNav;
	var $ConfigData;
	var $ConfigTabBadge;
	var $ConfigTabBadgeEmpty;
	var $ConfigContent;
	var $ConfigInfo;
	var $ConfigLicenses;
	var $ConfigTitle;
	var $ConfigTitleStatus;
	var $ConfigTable;
	var $ViewButton;
	var $LeaveConfigDialog;
	var $Body;

	// State
	var config = null;
	var filterText = '';
	var lastSection;
	var reloadTime;
	var updateTabInfo;
	var restored = false;
	var compactMode = false;
	var configSaved = false;
	var leaveTarget;
	this.currentSectionID = '';

	/*** READABLE LABELS (Oxide settings) ******************************************/

	// Words that keep their own spelling when an option name is turned into a label.
	var LABEL_WORDS = {
		nzb: 'NZB', nzbs: 'NZBs', url: 'URL', urls: 'URLs', tls: 'TLS', ssl: 'SSL', ip: 'IP', rss: 'RSS',
		par: 'par', par2: 'par2', api: 'API', ui: 'UI', id: 'ID', cpu: 'CPU', crc: 'CRC', mb: 'MB', kb: 'KB',
		ok: 'OK', http: 'HTTP', https: 'HTTPS', cert: 'certificate', dir: 'folder', cmd: 'command',
		ext: 'extensions', exts: 'extensions', max: 'maximum', min: 'minimum', sec: 'seconds', msec: 'milliseconds',
		dupe: 'duplicate', nntp: 'NNTP', unrar: 'UnRAR', rustyget: 'RustyGet', yenc: 'yEnc'
	};

	// Whole-name labels where splitting the name would read badly.
	var LABEL_OVERRIDES = {
		MainDir: 'Main folder', DestDir: 'Completed downloads folder', InterDir: 'Intermediate folder',
		NzbDir: 'Watch folder for NZB files', QueueDir: 'Queue folder', TempDir: 'Temporary folder',
		ScriptDir: 'Extensions folder', LogFile: 'Log file', ConfigTemplate: 'Configuration template',
		ControlIP: 'Listen address', ControlPort: 'Port', SecureControl: 'Use HTTPS', LogToFile: 'Write a log file', LogRotations: 'Rolled-over logs to keep',
		SecureCert: 'HTTPS certificate', SecureKey: 'HTTPS private key',
		CertCheck: 'Check news server certificates', CertStore: 'CA certificates',
		ControlUsername: 'Username', ControlPassword: 'Password', SevenZipCmd: '7-Zip command',
		UnrarCmd: 'UnRAR command', ParCheck: 'Par check', ParRepair: 'Par repair', ParScan: 'Par scan',
		ParBuffer: 'Par buffer', ParThreads: 'Par threads', ParPreset: 'Resource level',
		LiveParRepair: 'Repair while downloading (live par2)',
		DirectWrite: 'Write directly to the final file', ArticleCache: 'Article cache',
		FlushQueue: 'Flush queue to disk',
		DiskSpace: 'Pause below free disk space', KeepHistory: 'Keep history for',
		DirectUnpack: 'Unpack while downloading', DupeCheck: 'Duplicate check', DupeHealthPreset: 'Health sampling',
		RawArticle: 'Save raw articles', SkipWrite: 'Skip writing (testing)'
	};

	// Display names of some choices (the saved value stays the same).
	var VALUE_LABELS = {
		DupeHealthPreset: { weak: 'Weak / fast', medium: 'Medium / slower', hard: 'Hard / slowest', custom: 'Custom (see Expert)' },
		ParPreset: { low: 'Low', medium: 'Medium', large: 'Large', uber: 'Uber', detect: 'Detect', custom: 'Custom' }
	};

	function valueLabel(option, value)
	{
		var labels = VALUE_LABELS[option.name];
		var key = (value || '').toLowerCase();
		return labels && labels.hasOwnProperty(key) ?
			I18n.defaultValue('config_value_' + option.name.toLowerCase() + '_' + key, labels[key]) : null;
	}

	function humanizeOptionName(name)
	{
		if (LABEL_OVERRIDES.hasOwnProperty(name))
		{
			return LABEL_OVERRIDES[name];
		}
		var words = name
			.replace(/([a-z0-9])([A-Z])/g, '$1 $2')
			.replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2')
			.replace(/[_-]+/g, ' ')
			.split(' ')
			.filter(function(word) { return word !== ''; })
			.map(function(word, index)
			{
				var lower = word.toLowerCase();
				if (LABEL_WORDS.hasOwnProperty(lower))
				{
					return LABEL_WORDS[lower];
				}
				return index === 0 ? word.charAt(0).toUpperCase() + lower.substr(1) : lower;
			});
		var label = words.join(' ');
		return label.charAt(0).toUpperCase() + label.substr(1);
	}
	this.humanizeOptionName = humanizeOptionName;

	// "News-Servers" -> "News servers", "Incoming NZBs" stays; acronyms keep their case
	function sentenceCase(text)
	{
		return text.replace(/-/g, ' ').split(' ').filter(function(w) { return w !== ''; }).map(function(word, index)
		{
			if (/^[A-Z0-9]{2,}s?$/.test(word))
			{
				return word;
			}
			return index === 0 ? word.charAt(0).toUpperCase() + word.substr(1).toLowerCase() : word.toLowerCase();
		}).join(' ');
	}
	this.sentenceCase = sentenceCase;

	function optionLabel(option)
	{
		if (option.i18nCaption || option.name.indexOf(':') > -1)
		{
			return null; // extension options carry their own display names
		}
		var shortName = option.name.substr(option.name.indexOf('.') + 1);
		return humanizeOptionName(shortName);
	}

	this.init = function(options)
	{
		updateTabInfo = options.updateTabInfo;

		$Body = $('html, body');
		$ConfigNav = $('#ConfigNav');
		$ConfigData = $('#ConfigData');
		$ConfigTabBadge = $('#ConfigTabBadge');
		$ConfigTabBadgeEmpty = $('#ConfigTabBadgeEmpty');
		$ConfigContent = $('#ConfigContent');
		$ConfigInfo = $('#ConfigInfo');
		$ConfigLicenses = $('#ConfigLicenses');
		$ConfigTitle = $('#ConfigTitle');
		$ConfigTitleStatus = $('#ConfigTitleStatus');
		$ViewButton = $('#Config_ViewButton');
		$LeaveConfigDialog = $('#LeaveConfigDialog');
		$('#ConfigTable_filter').val('');
		compactMode = UISettings.read('$Config_ViewCompact', 'yes') == 'yes';
		setViewMode();

		$(window).bind('beforeunload', userLeavesPage);

		$ConfigNav.on('click', 'li > a', navClick);
		$('#ConfigInfo').on('click', '.config-overview-tile', function(event)
		{
			event.preventDefault();
			Config.showSection($(this).attr('data-target'), true);
		});
		$ConfigData.on('input change', '.config-list-detail input', refreshListRows);

		$ConfigTable = $('#ConfigTable');
		$ConfigTable.fasttable(
			{
				filterInput: $('#ConfigTable_filter'),
				filterClearButton: $("#ConfigTable_clearfilter"),
				filterInputCallback: filterInput,
				filterClearCallback: filterClear
			});
		I18n.subscribe(function onLangChange() {
			Config.redraw();
			Config._translateDescriptions();
		});
	}

	this.config = function()
	{
		return config;
	}

	this.show = function()
	{
		removeSaveBanner();
		$('#ConfigSaved').hide();
		$('#ConfigLoadInfo').show();
		$('#ConfigLoadServerTemplateError').hide();
		$('#ConfigLoadError').hide();
		$ConfigContent.hide();
		configSaved = false;
	}

	this.shown = function()
	{
		Options.loadConfig({
			complete: Config.buildPage,
			configError: Config.loadConfigError,
			serverTemplateError: Config.loadServerTemplateError
		});
	}

	this.hide = function()
	{
		Options.cleanup();
		config = null;
		serverEditing = 0;
		$ConfigContent.removeClass('server-editing server-pane-open server-page-open');
		$ConfigNav.children().not('.config-static').remove();
		$ConfigData.children().not('.config-static').remove();
	}

	this.loadConfigError = function(message, resultObj)
	{
		$('#ConfigLoadInfo').hide();
		$('#ConfigLoadError').show();
		if (resultObj && resultObj.error && resultObj.error.message)
		{
			message = resultObj.error.message;
		}
		$('#ConfigLoadErrorText').text(message);
	}

	this.loadServerTemplateError = function()
	{
		$('#ConfigLoadInfo').hide();
		$('#ConfigLoadServerTemplateError').show();
		var optConfigTemplate = Options.option('ConfigTemplate');
		$('#ConfigLoadServerTemplateErrorEmpty').toggle(optConfigTemplate === '');
		$('#ConfigLoadServerTemplateErrorNotFound').toggle(optConfigTemplate !== '');
		$('#ConfigLoadServerTemplateErrorWebDir').text(Options.option('WebDir'));
		$('#ConfigLoadServerTemplateErrorConfigFile').text(Options.option('ConfigFile'));
	}

	this.navigateTo = function(anchor)
	{
		$("a[href$='#" + anchor + "']").click();
	}

	function findOptionByName(name)
	{
		name = name.toLowerCase();

		for (var k=0; k < config.length; k++)
		{
			var sections = config[k].sections;
			for (var i=0; i < sections.length; i++)
			{
				var section = sections[i];
				for (var j=0; j < section.options.length; j++)
				{
					var option = section.options[j];
					if (!option.template &&
						((option.Name && option.Name.toLowerCase() === name) ||
						 (option.name && option.name.toLowerCase() === name)))
					{
						return option;
					}
				}
			}
		}
		return null;
	}
	this.findOptionByName = findOptionByName;

	function findOptionById(formId)
	{
		for (var k=0; k < config.length; k++)
		{
			var sections = config[k].sections;
			for (var i=0; i < sections.length; i++)
			{
				var section = sections[i];
				for (var j=0; j < section.options.length; j++)
				{
					var option = section.options[j];
					if (option.formId === formId)
					{
						return option;
					}
				}
			}
		}
		return null;
	}

	function findSectionById(sectionId)
	{
		if (sectionId === DUPE_SECTION.id)
		{
			return DUPE_SECTION;
		}
		for (var k=0; k < config.length; k++)
		{
			var sections = config[k].sections;
			for (var i=0; i < sections.length; i++)
			{
				var section = sections[i];
				if (section.id === sectionId)
				{
					return section;
				}
			}
		}
		return null;
	}

	this.processShortcut = function(key)
	{
		switch (key)
		{
			case 'Shift+F': $('#ConfigTable_filter').focus(); return true;
			case 'Shift+C': $('#ConfigTable_clearfilter').click(); return true;
		}
	}

	function formatDescription(text, hasAbout)
	{
		// replace option references
		var exp = /\<([A-Z0-9\.]*)\>/ig;
		text = text.replace(exp, '<a class="option" href="#" onclick="Config.scrollToOption(event, this)">$1</a>');

		// replace URLs
		exp = /(https?:\/\/[-A-Z0-9+&@#\/%?=~_|!:,.;]*[-A-Z0-9+&@#\/%=~_|])/ig;
		text = text.replace(exp, "<a href='$1'>$1</a>");

		// highlight first line or paragraph safely
		if (hasAbout)
		{
			text = text.replace(/\n/, '</span>\n');
		}
		else if (text.indexOf('\n\n') > -1)
		{
			text = text.replace(/\n\n/, '</span>\n\n');
		}
		else if (text.indexOf('\n') > -1)
		{
			text = text.replace(/\n/, '</span>\n');
		}
		else
		{
			text = text + '</span>';
		}
		text = '<span class="help-option-title">' + text;

		// Wrap NOTE / INFO labels
		text = text.replace(/NOTE: /g, '<span class="label label-warning text-uppercase">' + I18n.defaultValue('extman_note', 'NOTE') + '</span> ');
		text = text.replace(/INFO: /g, '<span class="label label-info text-uppercase">' + I18n.defaultValue('label_info_cap', 'INFO') + '</span>: ');

		// handle spoilers
		if (text.indexOf('INFO FOR DEVELOPERS:') > -1)
		{
			text = text.replace(/INFO FOR DEVELOPERS:\n/g,
				'<input class="btn btn-default btn-mini" data-i18n-value="btn_show_dev_info" value="" type="button" onclick="Config.showSpoiler(this)"><span class="hide">');
			text += '</span>';
		}
		if (text.indexOf('MORE INFO:') > -1)
		{
			text = text.replace(/MORE INFO:\n/g,
				'<input class="btn btn-default btn-mini" data-i18n-value="btn_show_more_info" value="" type="button" onclick="Config.showSpoiler(this)"><span class="hide">');
			text += '</span>';
		}

		return text;
	}

	/**
	 * Translates and reformats extension option descriptions after language change.
	 * The safe two-step approach: set as text (escapes all HTML), then
	 * selectively reintroduce formatting (<Option> links, URL links,
	 * NOTE/INFO labels, spoilers) via inner HTML.
	 */
	this._translateDescriptions = function()
	{
		$('pre.help-desc[data-i18n-key]').each(function() {
			var $pre = $(this);
			var key = $pre.attr('data-i18n-key');
			var text = I18n.translate(key);
			if (!text || text === key)
			{
				return;
			}

			$pre.text(text);

			// Step 2: get the safely-escaped text back
			var safeText = $pre.text();

			// Step 3: format the description using the shared helper
			safeText = formatDescription(safeText, false);

			$pre.html(safeText);
			$pre.closest('.control-group').find('.config-option-summary').text(text.split('\n')[0].trim());
		});
	}

	/*** GENERATE HTML PAGE *****************************************************************/

	function buildOptionsContent(section)
	{
		var html = '';

		var lastmultiid = 1;
		var firstmultioption = true;
		var hasoptions = false;

		for (var i=0, op=0; i < section.options.length; i++)
		{
			var option = section.options[i];
			if (SystemHealth.isHealthCheckEnabled())
			{
				option.check = SystemHealth.getCheck(SystemHealth.getSection(section.id), option.name);
			}
			if (!option.template)
			{
				if (section.multi && option.multiid !== lastmultiid)
				{
					// new set in multi section
					html += buildMultiRowEnd(section, lastmultiid, true, true);
					lastmultiid = option.multiid;
					firstmultioption = true;
				}
				if (section.multi && firstmultioption)
				{
					html += buildMultiRowStart(section, option.multiid, option);
					firstmultioption = false;
				}
				html += buildOptionRow(option, section);
				hasoptions = true;
				op++;
			}
		}

		if (section.multi)
		{
			html += buildMultiRowEnd(section, lastmultiid, false, hasoptions);
		}

		return html;
	}
	this.buildOptionsContent = buildOptionsContent;

	function buildMultiSetContent(section, multiid)
	{
		var html = '';
		var firstmultioption = true;
		var hasoptions = false;

		for (var i=0, op=0; i < section.options.length; i++)
		{
			var option = section.options[i];
			if (!option.template && option.multiid === multiid)
			{
				if (firstmultioption)
				{
					html += buildMultiRowStart(section, multiid, option);
					firstmultioption = false;
				}
				html += buildOptionRow(option, section);
				hasoptions = true;
				op++;
			}
		}
		html += buildMultiRowEnd(section, multiid, true, hasoptions);

		return html;
	}

	function buildOptionRow(option, section)
	{
		var value = option.value;
		if (option.value === null)
		{
			value = option.defvalue;
		}

		option.formId = (option.name.indexOf(':') == -1 ? 'S_' : '') + Util.makeId(option.name);

		// text (an extension's option names); the multi-set prefix markup is added below
		var caption = Util.textToHtml(option.caption);

		if (section.multi)
		{
			caption = '<span class="config-multicaption">' + caption.substring(0, caption.indexOf('.') + 1) + '</span>' + caption.substring(caption.indexOf('.') + 1);
		}

		// a readable label first, then the option's own name in mono so it still matches rustyget.conf
		option.label = optionLabel(option);
		// a list editor already names its server/category, so "Server1.Cipher" would repeat it
		if (option.label)
		{
			caption = '<span class="config-label">' + Util.textToHtml(option.label) + '</span>' +
				(listSectionPrefix(section.id) ? '' : '<span class="config-key">' + caption + '</span>');
		}

		// makeId removes colons, so label: IDs cannot collide with option controls.
		var i18nCaptionAttr = option.i18nCaption ? 'data-i18n="' + Util.textToAttr(option.i18nCaption) + '" ' : '';
		var html =
			'<div class="control-group ' + option.sectionId + (section.multi ? ' multiid' + option.multiid + ' multiset' : '') +
				(listSectionPrefix(section.id) ? ' config-list-detail list-closed' : '') + (!compactMode ? ' help-expanded' : '') + '">'+
				'<label class="control-label">' +
				'<a class="option-name" id="label:' + option.formId + '" href="#" data-optid="' + option.formId + '" ' + i18nCaptionAttr +
				'onclick="Config.scrollToOption(event, this)">' + caption + '</a>' +
				(option.value === null && !section.postparam && !option.commandopts ?
					' <a data-toggle="modal" href="#ConfigNewOptionHelp" class="label label-info text-uppercase">new</a>' : '') + '</label>'+
				'<div class="controls">';

		var about = option['about'] || '';
		if (about)
		{
			about = about.replace('\n', ' ') + '\n';
		}
		var rawDescription = option['description'] || '';
		var description;
		if (option.extKeyPrefix && about && !option.descriptionKey)
		{
			var aboutText = option.about;
			if (aboutText)
			{
				aboutText = aboutText.replace('\n', ' ');
			}
			aboutText = I18n.defaultValue(option.extKeyPrefix + 'about', aboutText) + '\n';
			var bodyText = option.body || rawDescription;
			if (bodyText)
			{
				bodyText = bodyText.replace(/\n+$/, '');
			}
			bodyText = I18n.defaultValue(option.extKeyPrefix + 'description', bodyText);
			description = aboutText + (bodyText || '') + '\n\n';

			var reqs = option.requirements || [];
			if (reqs.length > 0)
			{
				var joinedReqs = '';
				for (var rj = 0; rj < reqs.length; rj++)
				{
					if (reqs[rj])
					{
						if (joinedReqs) joinedReqs += '\n';
						joinedReqs += reqs[rj];
					}
				}
				var translatedReqs = I18n.defaultValue(option.extKeyPrefix + 'requirements', joinedReqs);
				var reqLines = translatedReqs.split('\n');
				for (var rk = 0; rk < reqLines.length; rk++)
				{
					if (reqLines[rk])
					{
						description += 'NOTE: ' + reqLines[rk] + '\n';
					}
				}
			}
		}
		else
		{
			description = option.descriptionKey ? rawDescription : (about + rawDescription);
			if (option.descriptionKey) {
				description = I18n.defaultValue(option.descriptionKey, description);
			} else {
				var optName = (option.name || '').replace(/\./g, '_').toLowerCase();
				var parts = optName.split('_');
				var first = parts.length > 0 ? parts[0].replace(/[0-9]+$/, '') : '';
				var rest = parts.length > 1 ? parts.slice(1).join('_') : '';
				var baseKey = (option.keyPrefix || 'config_desc_') + first;
				var fullKey = baseKey + (rest ? '_' + rest : '');
				description = I18n.defaultValue(fullKey, I18n.defaultValue(baseKey, description));
			}
		}

		if (option.nocontent)
		{
			option.type = 'info';
			html +=	'<div class="" id="' + option.formId + '"/>';
		}

		else if (option.select.length > 1)
		{
			option.type = 'switch';
			var isToggle = !section.postparam && option.select.length === 2 &&
				option.select.map(function(v) { return v.toLowerCase(); }).sort().join() === 'no,yes';
			if (isToggle)
			{
				var isOn = !!value && value.toLowerCase() === 'yes';
				html +=	'<div class="btn-group btn-switch is-toggle" role="checkbox" tabindex="0" aria-checked="' + isOn +
					'" aria-labelledby="label:' + option.formId + '" id="' + option.formId + '" onclick="Config.toggleClick(this, event)">';
			}
			else
			{
				html +=	'<div class="btn-group btn-switch" role="group" aria-labelledby="label:' + option.formId + '" id="' + option.formId + '">';
			}

			var valfound = false;
			for (var j=0; j < option.select.length; j++)
			{
				var pvalue = option.select[j];
				var pvalueDisplay = pvalue;
				if (pvalue.toLowerCase() === 'yes' || pvalue.toLowerCase() === 'no') {
					var labelKey = pvalue.toLowerCase() === 'yes' ? 'label_yes_cap' : 'label_no_cap';
					pvalueDisplay = I18n.defaultValue(labelKey, pvalueDisplay);
				}
				else if (option.i18nSelectPrefix)
				{
					pvalueDisplay = I18n.defaultValue(option.i18nSelectPrefix + pvalue.toLowerCase(), pvalueDisplay);
				}
				else if (valueLabel(option, pvalue))
				{
					pvalueDisplay = valueLabel(option, pvalue);
				}
				if (value && pvalue.toLowerCase() === value.toLowerCase())
				{
					html += '<input type="button" class="btn btn-primary" aria-pressed="true" value="' + Util.textToAttr(pvalueDisplay) + '" data-value="' + Util.textToAttr(pvalue) + '" onclick="Config.switchClick(this)">';
					valfound = true;
				}
				else
				{
					html += '<input type="button" class="btn btn-default" aria-pressed="false" value="' + Util.textToAttr(pvalueDisplay) + '" data-value="' + Util.textToAttr(pvalue) + '" onclick="Config.switchClick(this)">';
				}
			}
			if (!valfound)
			{
				var valueDisplay = value;
				if (value && (value.toLowerCase() === 'yes' || value.toLowerCase() === 'no')) {
					valueDisplay = I18n.defaultValue('config_value_' + value.toLowerCase(), valueDisplay);
				}
				else if (value && option.i18nSelectPrefix)
				{
					valueDisplay = I18n.defaultValue(option.i18nSelectPrefix + value.toLowerCase(), valueDisplay);
				}
				html += '<input type="button" class="btn btn-primary" aria-pressed="true" value="' + Util.textToAttr(valueDisplay) + '" data-value="' + Util.textToAttr(value) + '" onclick="Config.switchClick(this)">';
			}

			html +='</div>';
			if (isToggle)
			{
				var groupStart = html.lastIndexOf('<div class="btn-group btn-switch is-toggle"');
				html = html.substr(0, groupStart) + html.substr(groupStart).replace(/<input type="button" /g, '<input type="button" tabindex="-1" aria-hidden="true" ');
			}

		}
		else if (option.select.length === 1)
		{
			var unitStr = option.select[0];

			// Build i18n key from unit string:
			// - Convert to lowercase
			// - Remove all non-alphanumeric characters (spaces, slashes, etc.)
			// Examples: "MB/s" -> "config_unit_mbs", "hours or minutes" -> "config_unit_hoursorminutes"
			var unitKey = 'config_unit_' + unitStr.toLowerCase().replace(/[^a-z0-9]/g, '');

			// Try translation first, fallback to original unit string
			var translatedUnit = I18n.defaultValue(unitKey, unitStr);

			option.type = 'numeric';
			html += '<div class="input-append">'+
				'<input type="text" id="' + option.formId + '" value="' + Util.textToAttr(value) + '" class="editnumeric">'+
				'<span class="add-on text-lowercase">'+ Util.textToHtml(translatedUnit) +'</span>'+
				'</div>';
		}
		else if (option.caption.toLowerCase().indexOf('password') > -1 &&
			option.name.toLowerCase() !== '*unpack:password')
		{
			option.type = 'password';
			html += '<div class="password-field input-append">' +
				'<input type="password" id="' + option.formId + '" value="' + Util.textToAttr(value) + '" class="editsmall">'+
				'<span class="add-on">'+
				'<label class="checkbox">'+
				'<input type="checkbox" onclick="Config.togglePassword(this, \'' + option.formId + '\')" /> <span data-i18n="btn_show"></span>' +
				'</label>'+
				'</span>'+
				'</div>';
		}
		else if (option.caption.toLowerCase().indexOf('username') > -1 ||
			(option.caption.indexOf('IP') > -1 && option.name.toLowerCase() !== 'authorizedip'))
		{
			option.type = 'text';
			html += '<input type="text" id="' + option.formId + '" value="' + Util.textToAttr(value) + '" class="editsmall">';
		}
		else if (option.editor)
		{
			option.type = 'text';
			html += '<table class="editor"><tr><td>';
			html += '<input type="text" id="' + option.formId + '" value="' + Util.textToAttr(value) + '">';
			html += '</td><td>';
			html += '<button type="button" id="' + option.formId + '_Editor" class="btn btn-default" ' + (option.editor.i18n ? 'data-i18n="' + option.editor.i18n + '" ' : '') + 'onclick="' + option.editor.click + '($(\'input\', $(this).closest(\'table\')).attr(\'id\'))">' + (option.editor.i18n ? '' : option.editor.caption) + '</button>';
			html += '</td></tr></table>';
		}
		else
		{
			option.type = 'text';
			html += '<input type="text" id="' + option.formId + '" value="' + Util.textToAttr(value) + '" class="editlarge">';
		}

		var plain = plainHelp(option);
		if (plain && !section.postparam)
		{
			description = plain + '\n\n' + (description || '');
		}

		if (description) 
		{
			var htmldescr = description;
			htmldescr = htmldescr.replace(/NOTE: do not forget to uncomment the next line.\n/, '');

			// an extension's description (from its manifest) is text: everything
			// but the <OptionName> references formatDescription links is escaped
			htmldescr = htmldescr.replace(/&/g, '&amp;');
			htmldescr = htmldescr.split(/(<[A-Z0-9.]*>)/i).map(function(part, i) {
				return i % 2 ? part : part.replace(/</g, '&lt;').replace(/>/g, '&gt;');
			}).join('');

			// add extra new line after Examples not ended with dot
			htmldescr = htmldescr.replace(/Example:.*/g, function (match) {
				return match + (Util.endsWith(match, '.') ? '' : '\n');
			});

			htmldescr = formatDescription(htmldescr, option.about);

			if (option.descriptionKey)
			{
				htmldescr = '<pre class="help-desc" data-i18n-key="' + Util.textToAttr(option.descriptionKey) + '">' + htmldescr + '</pre>';
			}
			else
			{
				htmldescr = '<pre class="help-desc">' + htmldescr + '</pre>';
			}

			if (section.multi)
			{
				// replace strings like "TaskX.Command" and "Task1.Command"
				htmldescr = htmldescr.replace(new RegExp(section.multiprefix + '[X|1]\.', 'g'), '');
			}

			html = html.replace('class="control-group ', 'class="control-group config-has-help ');
			html += '<button type="button" class="btn config-help-toggle" aria-expanded="' + (!compactMode) +
				'" aria-controls="help:' + option.formId + '" aria-label="' + Util.textToAttr(I18n.translate('btn_quick_help') + ': ' + option.caption) +
				'" onclick="Config.toggleHelp(this)"><i class="material-icon" aria-hidden="true">help</i></button>' +
				'<div class="help-block" id="help:' + option.formId + '">' + htmldescr + '</div>';
		}

		if (option.check)
		{
			html += makeOptionCheckSection(option.check);
		}

		html += '</div>';
		if (!section.postparam && !option.nocontent && !option.commandopts)
		{
			html = html.replace('class="control-group ', 'class="control-group config-option-tile ');
		}
		if (description && !section.postparam && !option.nocontent && !option.commandopts)
		{
			var summary = description.split('\n')[0].trim();
			if (section.multi)
			{
				summary = summary.replace(new RegExp(section.multiprefix + '[X1]\\.', 'g'), '');
			}
			html += '<div class="config-option-summary">' + Util.textToHtml(summary) + '</div>';
		}
		html += '</div>';

		return html;
	}

	function makeOptionCheckSection(check) {
		let section = '<div class="option__check-section">';

		if (check.Severity == "Error")
			section += '<span class="option-alert alert alert-error"><i class="option-alert__icon material-icon">error</i><span>' + Util.textToHtml(check.Message) + '</span></span>';
		else if (check.Severity == "Warning")
			section += '<span class="option-alert alert alert-warning"><i class="option-alert__icon material-icon">warning</i><span>' + Util.textToHtml(check.Message) + '</span></span>';
		else if (check.Severity == "Info")
			section += '<span class="option-alert alert alert-success"><i class="option-alert__icon material-icon">info</i><span>' + Util.textToHtml(check.Message) + '</span></span>';

		section += '</div>';

		return section;
	}

	function buildMultiRowStart(section, multiid, option)
	{
		var name = option.caption;
		var setname = name.substr(0, name.indexOf('.')) || option.prefix + option.multiid;
		if (listSectionPrefix(section.id))
		{
			return '<div class="config-settitle config-list-row ' + section.id + ' multiid' + multiid +
				' multiset" data-section="' + section.id + '" data-multiid="' + multiid + '"><strong class="config-list-name"></strong>' +
				'<span class="config-list-host"></span><span class="config-list-status"></span>' +
				'<span class="config-list-connections"></span><span class="config-list-health"></span>' +
				'<button type="button" class="btn config-button config-list-edit" data-multiid="' + multiid +
				'" aria-expanded="false" onclick="Config.editList(this)"></button></div>';
		}
		var html = '<div class="config-settitle ' + section.id + ' multiid' + multiid + ' multiset">' + setname + '</div>';
		return html;
	}

	function buildMultiRowEnd(section, multiid, hasmore, hasoptions)
	{
		var option = section.options[0];
		var name = option.caption;
		var setname = name.substr(0, name.indexOf('1')) || option['prefix'] || '';
		var html = '';

		if (hasoptions)
		{
			html += '<div class="' + section.id + ' multiid' + multiid + ' multiset multiset-toolbar' +
				(listSectionPrefix(section.id) ? ' config-list-detail list-closed' : '') + '">';
			html += '<button type="button" class="btn btn-default config-button config-delete" data-multiid="' + multiid + '" ' +
				'onclick="Config.deleteSet(this, \'' + setname + '\',\'' + section.id + '\')">' +
				'<span data-i18n="btn_delete"></span> ' + setname + multiid + '</button>';
			html += ' <button type="button" class="btn btn-default config-button" data-multiid="' + multiid + '" ' +
				'onclick="Config.moveSet(this, \'' + setname + '\',\'' + section.id + '\', \'up\')">' +
				'<span data-i18n="btn_move_up"></span></button>';
			html += ' <button type="button" class="btn btn-default config-button" data-multiid="' + multiid + '" ' +
				'onclick="Config.moveSet(this, \'' + setname + '\',\'' + section.id + '\', \'down\')">' +
				'<span data-i18n="btn_move_down"></span></button>';
			if (setname.toLowerCase() === 'feed')
			{
				html += ' <button type="button" class="btn btn-default config-button" data-multiid="' + multiid + '" ' +
					'onclick="Config.previewFeed(this, \'' + setname + '\',\'' + section.id + '\')">' +
					'<span data-i18n="btn_preview_feed"></span></button>';
			}
			if (setname.toLowerCase() === 'server')
			{
				html = html.replace('multiset-toolbar', 'multiset-toolbar config-server-toolbar');
				html += '<div class="config-server-actions"><button type="button" class="btn btn-primary config-button config-server-save" ' +
					'data-multiid="' + multiid + '" onclick="Config.saveServer(this.getAttribute(\'data-multiid\'))"><span data-i18n="config_server_save">Save server</span></button> ' +
					'<button type="button" class="btn btn-default config-server-close" onclick="Config.closeServerEditor()">' +
					'<span data-i18n="config_server_back">Back to servers</span></button>' +
					'<span class="config-server-status" role="status" aria-live="polite" hidden></span></div>';
				html += ' <button type="button" class="btn btn-default config-button" data-multiid="' + multiid + '" ' +
					'onclick="Config.testConnection(this, \'' + setname + '\',\'' + section.id + '\')">' +
					'<span data-i18n="btn_test_connection"></span></button>';
				html += ' <button type="button" class="btn btn-default config-button" data-multiid="' + multiid + '" ' +
					'onclick="Config.serverStats(this, \'' + setname + '\',\'' + section.id + '\')">' +
					'<span data-i18n="btn_volume_statistics"></span></button>';
			}
			html += '<hr/></div>';
			html += '</div>';
		}

		if (!hasmore)
		{
			html += '<div class="' + section.id + '">';
			if (setname.toLowerCase() === 'server')
			{
				html += '<button type="button" id="Config_OptimizeTls" class="btn btn-default" ' +
					'onclick="TlsCipherDialog.showModal()"><span data-i18n="tls_cipher_title">Optimize TLS ciphers</span></button> ';
			}
			html += '<button type="button" class="btn btn-default config-add ' + section.id + ' multiset" ' +
				'onclick="Config.addSet(\'' + setname + '\',\'' + section.id + '\')">' +
				'<span data-i18n="label_add"></span> ' +
				(hasoptions ? '<span data-i18n="label_another"></span> ' : '') +
				setname + '</button>';
			html += '</div>';
		}

		return html;
	}

	this.buildPage = function(_config)
	{
		config = _config;

		extendConfig();

		$ConfigNav.children().not('.config-static').remove();
		$ConfigData.children().not('.config-static').remove();

		for (var k=0; k < config.length; k++)
		{
			if (k == 1)
			{
				$ConfigNav.append('<li class="divider"></li>');
			}
			var conf = config[k];
			var added = false;
			for (var i=0; i < conf.sections.length; i++)
			{
				var section = conf.sections[i];
				if (!section.hidden)
				{
					var html = $('<li>');
					var categoryName = I18n.defaultValue(('config_section_' + section.name.replace(/ /g, '_').replace(/-/g, '_')).toLowerCase(), section.name);
					var link = $('<a href="#' + section.id + '"></a>').text(categoryName);
					if (SystemHealth.isHealthCheckEnabled())
					{
						var errorBadges = SystemHealth.makeBadges(SystemHealth.getSection(section.id));
						if (errorBadges) link.append(errorBadges.addClass('config-health-badges'));
					}

					html.append(link);

					$ConfigNav.append(html);
					var content = buildOptionsContent(section);
					$ConfigData.append(content);
					added = true;
				}
			}
			if (!added)
			{
				var categoryName = I18n.defaultValue(('config_section_' + conf.name.replace(/ /g, '_').replace(/-/g, '_')).toLowerCase(), conf.name);
				var html = $('<li></li>').append($('<a href="#' + conf.id + '"></a>').text(categoryName));
				$ConfigNav.append(html);
			}
		}

		$ConfigNav.append('<li class="divider"></li>');
		$ConfigNav.append('<li><a class="text-uppercase" href="#Config-Licenses" data-i18n="config_title_licenses"></a></li>');

		applyLayout();
		notifyChanges();

		$ConfigNav.append('<li class="divider hide ConfigSearch"></li>');
		$ConfigNav.append('<li class="hide ConfigSearch"><a href="#Search" data-i18n="search_results"></a></li>');

		I18n.translatePage($ConfigNav);
		I18n.translatePage($ConfigData);
		groupNav();
		addRenamingControl();
		addListenAllControl();
		addHttpsControls();
		addCaControls();
		ensureModeSwitch();
		enhanceControls();
		snapshotShownValues();
		refreshChangeState();
		applyMode();
		refreshListRows();
		if (typeof TlsCipherDialog !== 'undefined') TlsCipherDialog.refreshAvailability();
		$ConfigNav.toggleClass('long-list', $ConfigNav.children().length > 20);

		Config.showSection('Config-Info', false);

		if (filterText !== '')
		{
			filterInput(filterText);
		}

		$('#ConfigLoadInfo').hide();
		$ConfigContent.show();
	}

	function extendConfig()
	{
		// This part must run every time to translate buttons!
		for (var i=0; i < config.length; i++)
		{
			var conf = config[i];
			for (var j=0; j < conf.sections.length; j++)
			{
				var section = conf.sections[j];
				for (var k=0; k < section.options.length; k++)
				{
					var option = section.options[k];
					var optname = option.name.toLowerCase();
					if (optname.indexOf('.filter') > -1)
					{
						option.editor = { caption: I18n.translate('btn_change'), i18n: 'btn_change', click: 'Config.editFilter' };
					}
				}
			}
		}
	}

	function notifyChanges()
	{
		for (var k=0; k < config.length; k++)
		{
			var sections = config[k].sections;
			for (var i=0; i < sections.length; i++)
			{
				var section = sections[i];
				for (var j=0; j < section.options.length; j++)
				{
					var option = section.options[j];
					if (option.onchange && !option.template)
					{
						option.onchange(option);
					}
				}
			}
		}
	}

	function scrollOptionIntoView(optFormId)
	{
		var option = findOptionById(optFormId);

		// following a search result or help link to an option the mode hides shows that mode
		if (option && !inMode(modeOfOption(option)))
		{
			Config.setMode(modeOfOption(option));
		}

		// switch to tab and scroll the option into view
		Config.showSection(pageIdOf(option), false);
		$('#' + optFormId).closest('details.config-card__more').prop('open', true);
		if (listSectionPrefix(option.sectionId))
		{
			setListEditor(option.sectionId, option.multiid, true);
		}

		var element = $('#' + option.formId);
		// a hidden input (such as Cipher behind its menu) scrolls to what shows in its place
		if (!element.is(':visible'))
		{
			var shown = element.siblings(':visible').first();
			element = shown.length ? shown : element.closest(':visible');
		}
		var smallScreen = $(window).width() <= 768;
		var parent = smallScreen ? $('html,.config__main') : $('.config__main');

		var offsetY = 30;
		if ($('body').hasClass('navfixed')) {
			offsetY = 10;
		}
		// a server option shown in the side pane scrolls inside the pane, not the page
		var pane = element.closest('.config-pane__body');
		if (pane.length)
		{
			parent = pane;
			offsetY = 16;
		}
		parent.animate({ scrollTop: parent.scrollTop() + element.offset().top - parent.offset().top - offsetY }, { duration: 'slow', easing: 'swing' });
	}

	this.toggleClick = function(group, event)
	{
		if ($(group).attr('aria-disabled') === 'true')
		{
			return;
		}
		// a click on one of the hidden Yes/No buttons has already switched through its own handler
		var onButton = event && event.target && $(event.target).is('input.btn');
		var target = $('.btn', group).filter(function() { return !$(this).hasClass('btn-primary'); }).first();
		if (!onButton && target.length)
		{
			Config.switchClick(target[0]);
		}
		$(group).attr('aria-checked', toggleIsOn(group) ? 'true' : 'false');
		refreshChangeState();
	}

	/*** SETTINGS LAYOUT: cards, advanced folds, the Dupe fleets page ***********/

	// The configuration file keeps its sections; the page groups their rows into
	// titled cards, folds rarely changed options away, and gathers every dupe fleet
	// option onto one page. Only the DOM moves: values, health and saving still go
	// through each option's own section. Options not listed stay in an untitled
	// card at the top of their section, so nothing new can go missing.
	var DUPE_SECTION = { id: 'DUPE_FLEETS', name: 'DUPE FLEETS', caption: 'Dupe fleets', options: [], virtual: true };

	var LAYOUT = {
		DUPE_FLEETS: [
			{ key: 'repair', title: 'Fleet repair', text: 'How copies of the same release repair each other while they download.',
				options: ['DupeCheck', 'DupeWatch', 'DupeArticleFallback', 'DupeBorrowFirst', 'DupeStreamDecompress', 'DupeStreamTimeout', 'HealthCheck'] },
			{ key: 'health', title: 'Health sampling', text: 'How much of each copy is checked before a primary is picked and donors are ranked.',
				options: ['DupeHealthPreset', 'DupeMinAlive'],
				advanced: ['DupeHealthPercent', 'DupeHealthMin', 'DupeHealthMax', 'DupeBodyChecks', 'DupeBodyPercent',
					'DupeHealthBudget', 'DupeCheckConcurrency', 'DupeCheckConnections'],
				moreLabel: 'Custom sampling' }
		],
		INCOMING_NZBS: [
			{ key: 'folder', title: 'NZB folder', text: 'Files dropped into the NZB folder are picked up and queued.',
				options: ['NzbDirInterval', 'NzbDirFileAge', 'NzbDirArchiveScan', 'NzbDirArchiveAction', 'AppendCategoryDir'] }
		],
		DOWNLOAD_QUEUE: [
			{ key: 'queue', title: 'Queue', options: ['PostStrategy', 'KeepHistory', 'FeedHistory', 'FlushQueue'] },
			{ key: 'disk', title: 'Writing to disk', options: ['ArticleCache', 'DirectWrite', 'WriteBuffer', 'DiskSpace', 'NzbCleanupDisk'],
				advanced: ['SkipWrite', 'RawArticle'] },
			{ key: 'names', title: 'File names', options: ['FileNaming', 'PostDownloadRename', 'RenameAfterUnpack', 'ReorderFiles', 'RenameIgnoreExt'] }
		],
		CONNECTION: [
			{ key: 'speed', title: 'Speed and quotas', options: ['DownloadRate', 'DailyQuota', 'MonthlyQuota', 'QuotaStartDay'] },
			{ key: 'articles', title: 'Articles', options: ['ArticleRetries', 'ArticleInterval', 'ArticleTimeout'], advanced: ['ArticleReadChunkSize'] },
			{ key: 'urls', title: 'Fetching NZB links', options: ['UrlConnections', 'UrlForce', 'UrlRetries', 'UrlInterval', 'UrlTimeout', 'RemoteTimeout'] }
		],
		CHECK_AND_REPAIR: [
			// one card: par2 is the fallback when no twin or alt in the fleet has the bytes
			{ key: 'par', title: 'Check and repair', text: 'Used when no twin or alt in the fleet has the missing bytes.',
				options: ['ParPreset', 'LiveParRepair', 'ParCheck', 'ParRepair', 'ParScan', 'DirectRename', 'ParRename', 'RarRename', 'HardLinking',
					'HardLinkingIgnoreExt', 'ParTimeLimit', 'ParIgnoreExt'],
				advanced: ['ParBuffer', 'ParThreads', 'ParPauseQueue'],
				moreLabel: 'Custom resources' }
		],
		UNPACK: [
			{ key: 'unpack', title: 'Unpack', options: ['Unpack', 'DirectUnpack', 'UnpackPassFile', 'UseTempUnpackDir', 'UnpackPauseQueue'],
				advanced: ['UnpackMaxFiles', 'UnpackMaxDepth', 'UnpackMaxFolders'] },
			{ key: 'cleanup', title: 'Cleanup', options: ['UnpackCleanupDisk', 'ExtCleanupDisk', 'UnpackIgnoreExt'] }
		],
		PATHS: [
			{ key: 'folders', title: 'Folders', options: ['MainDir', 'DestDir', 'InterDir', 'NzbDir', 'TempDir', 'QueueDir'],
				advanced: ['WebDir', 'LockFile', 'LogFile', 'ConfigTemplate', 'RequiredDir'] }
		],
		SECURITY: [
			{ key: 'access', title: 'Web access', options: ['ControlIP', 'ControlPort', 'ControlUsername', 'ControlPassword', 'FormAuth', 'AuthorizedIP'] },
			{ key: 'users', title: 'Extra users', text: 'A restricted user can watch and change the queue but not settings. An add user can only add NZBs.',
				options: ['RestrictedUsername', 'RestrictedPassword', 'AddUsername', 'AddPassword'] },
			{ key: 'https', title: 'HTTPS', text: 'HTTPS is answered on the same port as the web interface.',
				options: ['SecureControl', 'SecureCert', 'SecureKey'] },
			{ key: 'system', title: 'System', options: ['DaemonUsername', 'UMask', 'JsonP', 'CertCheck', 'CertStore'] }
		],
		LOGGING: [
			{ key: 'file', title: 'Log file', options: ['LogToFile', 'LogRotations', 'LogBuffer', 'SystemHealthCheck'],
				text: 'Moved aside every 10 MB, named by day and number, and gzipped; the newest rolled-over files are kept. Messages of each download are written here too.' },
			{ key: 'levels', title: 'Message levels', options: ['ErrorTarget', 'WarningTarget', 'InfoTarget', 'DetailTarget', 'DebugTarget'] },
			{ key: 'debug', title: 'Troubleshooting', options: [], advanced: ['CrashTrace', 'CrashDump', 'TimeCorrection'] }
		]
	};

	function layoutRow(name)
	{
		var option = findOptionByName(name);
		if (!option || option.template)
		{
			return null;
		}
		var row = $('#' + option.formId, $ConfigData).closest('.control-group');
		return row.length ? { option: option, row: row } : null;
	}

	function buildCard(sectionId, card)
	{
		var $card = $('<section class="config-card"></section>').addClass(sectionId).attr('data-card', card.key);
		if (card.title)
		{
			var head = $('<header class="config-card__head"></header>');
			head.append($('<h3 class="config-card__title"></h3>').text(I18n.defaultValue('config_card_' + sectionId.toLowerCase() + '_' + card.key, card.title)));
			if (card.text)
			{
				head.append($('<p class="config-card__text"></p>').text(I18n.defaultValue('config_card_' + sectionId.toLowerCase() + '_' + card.key + '_text', card.text)));
			}
			$card.append(head);
		}
		var rows = $('<div class="config-card__rows"></div>').appendTo($card);
		function move(names, target)
		{
			var moved = 0;
			(names || []).forEach(function(name)
			{
				var found = layoutRow(name);
				if (!found)
				{
					return;
				}
				if (sectionId !== found.option.sectionId)
				{
					found.row.removeClass(found.option.sectionId).addClass(sectionId);
					found.option.pageId = sectionId;
					DUPE_SECTION.options.push(found.option);
				}
				if (sectionId === DUPE_SECTION.id)
				{
					// every label here would start with "Duplicate": the page title says it once
					found.row.find('.config-label').first().text(function(i, text) { return text.replace(/^Duplicate\s+(\S)/, function(m, c) { return c.toUpperCase(); }); });
				}
				found.row.addClass('in-card').appendTo(target);
				moved++;
			});
			return moved;
		}
		move(card.options, rows);
		if (card.advanced && card.advanced.length)
		{
			var more = $('<details class="config-card__more"></details>');
			var summary = $('<summary class="config-card__more-toggle"></summary>').appendTo(more);
			var body = $('<div class="config-card__rows"></div>').appendTo(more);
			var count = move(card.advanced, body);
			if (count)
			{
				summary.text((card.moreLabel ? I18n.defaultValue('config_card_' + sectionId.toLowerCase() + '_' + card.key + '_more', card.moreLabel) :
					I18n.defaultValue('config_card_more', 'Advanced')) + ' (' + count + ')');
				$card.append(more);
			}
		}
		return $card;
	}

	function applyLayout()
	{
		DUPE_SECTION.options = [];
		$('.config-card', $ConfigData).remove();
		Object.keys(LAYOUT).forEach(function(sectionId)
		{
			var section = sectionId === DUPE_SECTION.id ? DUPE_SECTION : findSectionById(sectionId);
			if (!section || section.hidden)
			{
				return;
			}
			// the cards take the place of the section's first row, which may itself move into one
			var marker = $('<span class="config-layout-marker" hidden></span>');
			var first = $ConfigData.children('.control-group.' + sectionId).not('.config-static').first();
			if (first.length)
			{
				first.before(marker);
			}
			else
			{
				$ConfigData.append(marker);
			}
			var cards = LAYOUT[sectionId].map(function(card) { return buildCard(sectionId, card); })
				.filter(function($card) { return $card.find('.control-group').length > 0; });
			if (cards.length === 0)
			{
				marker.remove();
				return;
			}
			// rows nobody listed: an untitled card first, in file order
			var leftover = $ConfigData.children('.control-group.' + sectionId).not('.config-static');
			var $cards = $();
			if (leftover.length)
			{
				var rest = $('<section class="config-card"></section>').addClass(sectionId).attr('data-card', 'other');
				$('<div class="config-card__rows"></div>').append(leftover.addClass('in-card')).appendTo(rest);
				$cards = $cards.add(rest);
			}
			cards.forEach(function($card) { $cards = $cards.add($card); });
			marker.replaceWith($cards);
			if (sectionId === DUPE_SECTION.id)
			{
				var link = $('<a></a>').attr('href', '#' + DUPE_SECTION.id).text(I18n.defaultValue('config_section_dupe_fleets', 'Dupe fleets'));
				$ConfigNav.append($('<li></li>').append(link));
			}
		});
		refreshLayoutHealth();
	}

	// The Dupe fleets page gets the same error badge as other sections, counted
	// from the rows it shows; folds open on anything that needs a look.
	function refreshLayoutHealth()
	{
		function setBadge(sectionId, errors)
		{
			var link = $ConfigNav.find('a').filter(function() { return $(this).attr('href') === '#' + sectionId; });
			link.children('.config-health-badges').remove();
			if (errors > 0)
			{
				link.append($('<span class="config-health-badges" style="margin-left: 5px;"></span>')
					.append($('<span class="badge badge-important" style="margin-left: 3px;"></span>').text(errors)));
			}
		}
		var moved = 0;
		var bySource = {};
		DUPE_SECTION.options.forEach(function(option)
		{
			if (option.check && option.check.Severity === 'Error')
			{
				moved++;
				bySource[option.sectionId] = (bySource[option.sectionId] || 0) + 1;
			}
		});
		if (SystemHealth.isHealthCheckEnabled())
		{
			// an error on a moved option counts where the option is shown, not where it is stored
			Object.keys(bySource).forEach(function(sectionId)
			{
				var health = SystemHealth.getSection(sectionId);
				setBadge(sectionId, health ? health.getErrorsCount() - bySource[sectionId] : 0);
			});
		}
		setBadge(DUPE_SECTION.id, moved);
		openFoldsThatMatter();
	}

	// a fold opens when something inside it is changed, invalid or has a health notice
	function openFoldsThatMatter()
	{
		$('details.config-card__more', $ConfigData).each(function()
		{
			if ($(this).find('.is-changed, .option-alert, .control-group.error').length)
			{
				this.open = true;
			}
		});
	}

	function pageIdOf(option)
	{
		return option.pageId || option.sectionId;
	}

	/*** SETTINGS OVERVIEW ******************************************************/

	function optionValueByName(name)
	{
		var option = findOptionByName(name);
		if (!option)
		{
			return '';
		}
		var control = $('#' + option.formId, $ConfigData);
		if (control.length)
		{
			return String(getOptionValue(option) || '');
		}
		return String(option.value === null ? option.defvalue : option.value);
	}

	function overviewTile(target, label, icon, value, sub, tone)
	{
		var tile = $('<a class="details-tile details-tile-link config-overview-tile" href="#"></a>').attr('data-target', target);
		tile.append($('<span class="details-tile-label"></span>').text(label)
			.append(icon ? $('<i class="material-icon" aria-hidden="true"></i>').text(icon) : null));
		// The parts are stacked lines; the spaces between them don't show but keep the tile's
		// text (its accessible name, a copy) from running words together ("active60").
		tile.append(' ', $('<span class="details-tile-value"></span>').text(value).addClass(tone ? 'is-' + tone : ''));
		if (sub)
		{
			tile.append(' ', $('<span class="details-tile-sub"></span>').text(sub));
		}
		return tile;
	}

	function renderOverview()
	{
		var box = $('#ConfigOverview');
		if (!box.length || !config)
		{
			return;
		}
		var t = function(key, text) { return I18n.defaultValue(key, text); };
		var yes = function(name) { return optionValueByName(name).toLowerCase() === 'yes'; };
		var tiles = $('<div class="details-tiles config-overview-tiles"></div>');

		// news servers
		var total = 0, active = 0, connections = 0;
		for (var id = 1; id < 100; id++)
		{
			if (!findOptionByName('Server' + id + '.Host'))
			{
				continue;
			}
			total++;
			if (optionValueByName('Server' + id + '.Active').toLowerCase() !== 'no')
			{
				active++;
				connections += parseInt(optionValueByName('Server' + id + '.Connections'), 10) || 0;
			}
		}
		tiles.append(overviewTile('NEWS-SERVERS', t('config_overview_servers', 'News servers'), 'arrow_right_alt',
			total ? active + ' / ' + total + ' ' + t('config_overview_active', 'active') : t('config_overview_none', 'None yet'),
			connections + ' ' + t('config_overview_connections', 'connections'), total && active ? 'good' : 'bad'));

		// where finished downloads go
		var dest = optionValueByName('DestDir');
		var inter = optionValueByName('InterDir');
		tiles.append(overviewTile('PATHS', t('config_overview_dest', 'Downloads go to'), 'arrow_right_alt',
			dest || '-', inter ? t('config_overview_inter', 'working in') + ' ' + inter : ''));

		// dupe fleets
		var fallback = optionValueByName('DupeArticleFallback').toLowerCase();
		if (fallback === 'yes')
		{
			fallback = 'article'; // older configurations: the daemon reads "yes" as article
		}
		var fleetValue = !yes('DupeCheck') ? t('config_overview_off', 'Off') :
			{ live: t('config_overview_live', 'Live repair'), stream: t('config_overview_stream', 'Stream repair'),
				article: t('config_overview_article', 'Article swap') }[fallback] || t('config_overview_dupe_only', 'Dupe check only');
		var fleetSub = [];
		if (yes('DupeBorrowFirst')) fleetSub.push(t('config_overview_borrow_first', 'borrow before par2'));
		var preset = optionValueByName('DupeHealthPreset').toLowerCase() || 'medium';
		fleetSub.push(t('config_overview_sampling', 'sampling: %s').replace('%s',
			{ weak: t('config_overview_sampling_weak', 'weak'), medium: t('config_overview_sampling_medium', 'medium'),
				hard: t('config_overview_sampling_hard', 'hard'), custom: t('config_overview_sampling_custom', 'custom') }[preset] || preset));
		tiles.append(overviewTile(DUPE_SECTION.id, t('config_section_dupe_fleets', 'Dupe fleets'), 'arrow_right_alt',
			fleetValue, fleetSub.join(' · '), yes('DupeCheck') ? 'good' : 'warn'));

		// repair and unpack
		var parCheck = optionValueByName('ParCheck');
		tiles.append(overviewTile('CHECK_AND_REPAIR', t('config_overview_par', 'Par check'), 'arrow_right_alt',
			parCheck ? parCheck.charAt(0).toUpperCase() + parCheck.slice(1) : '-',
			(yes('ParRepair') ? t('config_overview_repair_on', 'repair on') : t('config_overview_repair_off', 'repair off')) +
				' · ' + t('config_overview_par_level', 'level: %s').replace('%s', VALUE_LABELS.ParPreset[String(optionValueByName('ParPreset') || 'custom').toLowerCase()] || optionValueByName('ParPreset'))));
		tiles.append(overviewTile('UNPACK', t('config_overview_unpack', 'Unpack'), 'arrow_right_alt',
			!yes('Unpack') ? t('config_overview_off', 'Off') : yes('DirectUnpack') ? t('config_overview_direct', 'While downloading') : t('config_overview_after', 'After download'),
			yes('UnpackCleanupDisk') ? t('config_overview_cleanup', 'archives deleted after') : t('config_overview_keep', 'archives kept')));

		// speed
		var rate = parseInt(optionValueByName('DownloadRate'), 10) || 0;
		var quota = parseInt(optionValueByName('DailyQuota'), 10) || parseInt(optionValueByName('MonthlyQuota'), 10) || 0;
		tiles.append(overviewTile('CONNECTION', t('config_overview_speed', 'Speed limit'), 'arrow_right_alt',
			rate ? Util.formatSpeed(rate * 1024) : t('config_overview_unlimited', 'Unlimited'),
			quota ? t('config_overview_quota', 'quota set') : t('config_overview_no_quota', 'no quota')));

		// a tile leads only to a page the current mode shows
		tiles.children().each(function()
		{
			var item = $('a[href="#' + $(this).attr('data-target') + '"]', $ConfigNav).closest('li');
			$(this).toggle(!item.hasClass('mode-hidden'));
		});
		box.empty().append(tiles);
	}
	this.renderOverview = renderOverview;

	/*** SETTINGS MODES AND CONTROLS ********************************************/

	// Simple shows what nearly everyone changes; Advanced adds the rest of the everyday
	// options; Expert shows every option. An option not listed here is Expert. Options
	// of a list (servers, categories, feeds, tasks) are named without their number.
	var OPTION_MODES = {
		simple: [
			'DestDir',
			'InterDir',
			'NzbDir',
			'Server.Active',
			'Server.Name',
			'Server.Host',
			'Server.Backbone',
			'Server.Port',
			'Server.Encryption',
			'Server.Username',
			'Server.Password',
			'Server.Connections',
			'Category.Name',
			'Category.DestDir',
			'DupeCheck',
			'DupeArticleFallback',
			'DupeHealthPreset',
			'DownloadRate',
			'KeepHistory',
			'Unpack',
			'DirectUnpack',
			'UnpackCleanupDisk',
			'ControlUsername',
			'ControlPassword'
		],
		advanced: [
			'TempDir',
			'QueueDir',
			'MainDir',
			'Server.Priority',
			'Server.Optional',
			'Server.Retention',
			'Server.CertVerification',
			'Server.IpVersion',
			'Server.Notes',
			'Category.Unpack',
			'Category.Aliases',
			'Feed.Name',
			'Feed.URL',
			'Feed.Filter',
			'Feed.Interval',
			'Feed.Backlog',
			'Feed.PauseNzb',
			'Feed.Category',
			'Feed.CategorySource',
			'Feed.CertVerification',
			'Feed.Priority',
			'Task.Time',
			'Task.WeekDays',
			'Task.Command',
			'Task.Param',
			'DupeBorrowFirst',
			'DupeStreamDecompress',
			'DupeStreamTimeout',
			'DupeMinAlive',
			'HealthCheck',
			'AppendCategoryDir',
			'NzbDirInterval',
			'NzbDirFileAge',
			'NzbDirArchiveScan',
			'NzbDirArchiveAction',
			'FeedHistory',
			'ArticleCache',
			'DiskSpace',
			'PostStrategy',
			'FileNaming',
			'DailyQuota',
			'MonthlyQuota',
			'QuotaStartDay',
			'ArticleRetries',
			'ArticleTimeout',
			'UrlConnections',
			'ParCheck',
			'ParRepair',
			'ParScan',
			'ParTimeLimit',
			'UnpackPassFile',
			'UnpackPauseQueue',
			'ExtCleanupDisk',
			'ControlIP',
			'ControlPort',
			'FormAuth',
			'AuthorizedIP',
			'RestrictedUsername',
			'RestrictedPassword',
			'AddUsername',
			'AddPassword',
			'SecureControl',
			'SecureCert',
			'SecureKey',
			'LogToFile',
			'LogRotations',
			'LogBuffer',
			'ErrorTarget',
			'WarningTarget',
			'InfoTarget',
			'DetailTarget'
		]
	};

	// pages without options: the lowest mode that lists them
	var STATIC_PAGE_MODES = { 'Config-System': 'advanced', 'Config-Licenses': 'advanced' };

	var MODE_RANK = { simple: 0, advanced: 1, expert: 2 };
	var MODE_LABELS = { simple: 'Simple', advanced: 'Advanced', expert: 'Expert' };
	var settingsMode = 'simple';
	try
	{
		var storedMode = localStorage.getItem('SettingsMode');
		if (MODE_RANK.hasOwnProperty(storedMode))
		{
			settingsMode = storedMode;
		}
	}
	catch (e) { /* storage blocked: Simple */ }

	var modeIndex = null;
	function optionFamily(name)
	{
		return String(name).replace(/^([A-Za-z]+)[0-9]+\./, '$1.');
	}

	function modeOfOption(option)
	{
		if (!modeIndex)
		{
			modeIndex = {};
			OPTION_MODES.advanced.forEach(function(name) { modeIndex[name.toLowerCase()] = 'advanced'; });
			OPTION_MODES.simple.forEach(function(name) { modeIndex[name.toLowerCase()] = 'simple'; });
		}
		return modeIndex[optionFamily(option.name).toLowerCase()] || 'expert';
	}

	function inMode(mode)
	{
		return MODE_RANK[mode] <= MODE_RANK[settingsMode];
	}

	function allOptions(callback)
	{
		if (!config)
		{
			return;
		}
		for (var k=0; k < config.length; k++)
		{
			var sections = config[k].sections;
			for (var i=0; i < sections.length; i++)
			{
				var section = sections[i];
				if (section.hidden)
				{
					continue;
				}
				for (var j=0; j < section.options.length; j++)
				{
					var option = section.options[j];
					if (!option.template && option.formId)
					{
						callback(option, section);
					}
				}
			}
		}
	}

	// hide what the mode leaves out: rows, then empty folds, cards and pages
	function applyMode()
	{
		if (!$ConfigData)
		{
			return;
		}
		var pageHasRows = {};
		allOptions(function(option, section)
		{
			var mode = modeOfOption(option);
			var row = $(document.getElementById(option.formId)).closest('.control-group');
			var hidden = !inMode(mode);
			row.toggleClass('mode-hidden', hidden).attr('data-mode', mode);
			var tag = row.find('.config-mode-tag');
			if (mode === 'simple')
			{
				tag.remove();
			}
			else if (!tag.length)
			{
				row.find('.control-label').first().append($('<span class="config-mode-tag"></span>')
					.text(I18n.defaultValue('config_mode_' + mode, MODE_LABELS[mode])));
			}
			if (!hidden)
			{
				pageHasRows[pageIdOf(option)] = true;
				pageHasRows[section.id] = pageHasRows[section.id] || pageIdOf(option) === section.id;
			}
		});
		// an empty list (no feeds or tasks yet) still shows when its fields are in the mode,
		// so its add button can be reached
		for (var k=0; k < config.length; k++)
		{
			config[k].sections.forEach(function(section)
			{
				if (section.hidden)
				{
					return;
				}
				section.options.forEach(function(option)
				{
					if (option.template && inMode(modeOfOption(option)))
					{
						pageHasRows[section.id] = true;
					}
				});
			});
		}
		$('.control-group[data-virtual-mode]', $ConfigData).each(function()
		{
			var mode = $(this).attr('data-virtual-mode');
			// the menu stands in for options Expert shows one by one
			$(this).toggleClass('mode-hidden', !inMode(mode) || settingsMode === 'expert').attr('data-mode', mode);
		});
		$('.config-card', $ConfigData).each(function()
		{
			$(this).toggleClass('mode-empty', $(this).find('.control-group').not('.mode-hidden').length === 0);
		});
		$('details.config-card__more', $ConfigData).each(function()
		{
			$(this).toggleClass('mode-empty', $(this).find('.control-group').not('.mode-hidden').length === 0);
		});
		// list sections: the add button and summary rows follow the list's fields
		$('.config-list-row, .config-add', $ConfigData).each(function()
		{
			var sectionId = $(this).attr('data-section') || ($(this).attr('class').match(/\b(NEWS-SERVERS|CATEGORIES|RSS_FEEDS|SCHEDULER)\b/) || [])[1];
			if (sectionId)
			{
				$(this).closest('.control-group').toggleClass('mode-hidden', !pageHasRows[sectionId]);
			}
		});
		$('li', $ConfigNav).each(function()
		{
			var href = ($('> a', this).attr('href') || '').substr(1);
			if (!href || href === 'Search' || href === 'Config-Info' || href === 'Config-SystemInfo')
			{
				return;
			}
			var visible = STATIC_PAGE_MODES.hasOwnProperty(href) ? inMode(STATIC_PAGE_MODES[href]) :
				(findSectionById(href) ? !!pageHasRows[href] : true);
			$(this).toggleClass('mode-hidden', !visible);
		});
		// a group label with nothing left under it goes too
		$('li.config-nav-group', $ConfigNav).each(function()
		{
			var items = $(this).nextUntil('li.config-nav-group').not('.ConfigSearch, .divider');
			$(this).toggleClass('mode-hidden', items.length > 0 && items.not('.mode-hidden').length === 0);
		});
		$ConfigContent.attr('data-mode', settingsMode);
		$('#ConfigMode button').each(function()
		{
			var on = $(this).attr('data-mode') === settingsMode;
			$(this).toggleClass('is-active', on).attr('aria-checked', on ? 'true' : 'false').attr('tabindex', on ? '0' : '-1');
		});
		var current = Config.currentSectionID;
		if (current && $('a[href="#' + current + '"]', $ConfigNav).closest('li').hasClass('mode-hidden'))
		{
			Config.showSection('Config-Info', false);
		}
		else if (current === 'Config-Info')
		{
			renderOverview();
		}
		markLastControlGroup();
	}

	this.setMode = function(mode)
	{
		if (!MODE_RANK.hasOwnProperty(mode))
		{
			return;
		}
		settingsMode = mode;
		try { localStorage.setItem('SettingsMode', mode); } catch (e) { /* not remembered */ }
		applyMode();
	};

	this.mode = function()
	{
		return settingsMode;
	};

	function ensureModeSwitch()
	{
		if ($('#ConfigMode').length)
		{
			return;
		}
		var group = $('<div id="ConfigMode" class="config-mode" role="radiogroup"></div>')
			.attr('aria-label', I18n.defaultValue('config_mode_label', 'Settings mode'));
		['simple', 'advanced', 'expert'].forEach(function(mode)
		{
			group.append($('<button type="button" role="radio"></button>').attr('data-mode', mode)
				.text(I18n.defaultValue('config_mode_' + mode, MODE_LABELS[mode])));
		});
		group.on('click', 'button', function() { Config.setMode($(this).attr('data-mode')); });
		group.on('keydown', 'button', function(e)
		{
			var order = ['simple', 'advanced', 'expert'];
			var index = order.indexOf(settingsMode);
			if (e.key === 'ArrowRight' || e.key === 'ArrowDown') index = Math.min(order.length - 1, index + 1);
			else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') index = Math.max(0, index - 1);
			else return;
			e.preventDefault();
			Config.setMode(order[index]);
			group.find('button[data-mode="' + order[index] + '"]').focus();
		});
		$('.config-header-actions', $ConfigContent).prepend(group);
	}

	// Number options: limits, a slider scale and three to five presets with what each
	// one means. A slider appears when an option has limits; dots on it are the presets
	// and the thumb snaps to a dot it passes near. The help lists the presets.
	var NUMBER_META = {
		'Server.Connections': { min: 1, max: 100, presets: [
			[4, 'Light', 'Enough for slow lines and small accounts; downloads may not reach full speed.'],
			[20, 'Normal', 'Fills most connections up to 500 Mbit/s.'],
			[50, 'Fast', 'For gigabit lines. Your account must allow this many connections.'],
			[100, 'Maximum', 'Only if your provider allows it; extra connections are refused otherwise.']] },
		'Server.Retention': { min: 0, max: 6000, presets: [
			[0, 'Unlimited', 'Try every article on this server, however old.'],
			[1500, 'Block account', 'Skip articles older than about four years.'],
			[4000, 'Long', 'Typical of large providers today.']] },
		ArticleCache: { min: 0, max: 4096, scale: 'sqrt', presets: [
			[0, 'Off', 'Every article goes straight to disk: least memory, most disk writes.'],
			[200, 'Low', 'For machines with little memory.'],
			[500, 'Normal', 'Good for most machines.'],
			[2000, 'High', 'Fewer disk writes during fast downloads; uses up to 2 GB of memory.']] },
		WriteBuffer: { min: 0, max: 8192, scale: 'sqrt', presets: [
			[0, 'Off', 'Lowest memory; more small writes.'],
			[1024, 'Normal', 'Good for most disks.'],
			[4096, 'High', 'Fewer writes on slow or network disks; uses more memory per connection.']] },
		DiskSpace: { min: 0, max: 10000, scale: 'sqrt', presets: [
			[0, 'Never pause', 'Downloads continue until the disk is full.'],
			[250, 'Normal', 'Pause when less than 250 MB is free.'],
			[2000, 'Safe', 'Leave room for unpacking a large release.']] },
		KeepHistory: { min: 0, max: 365, scale: 'sqrt', presets: [
			[0, 'Off', 'Finished downloads leave the history at once; duplicate checks cannot see them.'],
			[7, 'Week', 'A short history.'],
			[30, 'Month', 'Normal.'],
			[365, 'Year', 'Remembers a year of downloads for duplicate checks; the queue file grows.']] },
		FeedHistory: { min: 0, max: 90, presets: [
			[1, 'Day', 'Old feed items may be fetched again.'],
			[7, 'Week', 'Normal.'],
			[30, 'Month', 'For feeds that repeat old items.']] },
		DownloadRate: { min: 0, max: 125000, scale: 'sqrt', presets: [
			[0, 'Unlimited', 'Use the whole line.'],
			[1250, '10 Mbit/s', 'Leaves most of a typical line free.'],
			[12500, '100 Mbit/s', 'A firm cap for shared lines.'],
			[62500, '500 Mbit/s', 'Caps only very fast lines.']] },
		ArticleRetries: { min: 0, max: 10, presets: [
			[0, 'None', 'Give up on a server at the first error and move on to the next one.'],
			[3, 'Normal', 'Recovers from short glitches.'],
			[10, 'Stubborn', 'Keeps retrying a flaky server; failures take longer to move on.']] },
		ArticleTimeout: { min: 5, max: 300, presets: [
			[15, 'Quick', 'Drops stalled connections fast; may cut off slow servers.'],
			[60, 'Normal', 'Good for most servers.'],
			[180, 'Patient', 'For slow or distant servers.']] },
		ArticleInterval: { min: 0, max: 60, presets: [
			[0, 'At once', 'Retry immediately.'],
			[10, 'Normal', 'Give the server a moment.'],
			[30, 'Gentle', 'For servers that throttle quick retries.']] },
		UrlConnections: { min: 1, max: 20, presets: [
			[1, 'One at a time', 'Gentlest on indexers.'],
			[4, 'Normal', 'Fetches a few NZB links at once.'],
			[10, 'Many', 'Faster bulk adds; some indexers limit this.']] },
		UrlRetries: { min: 0, max: 10, presets: [
			[0, 'None', 'One try per NZB link.'],
			[3, 'Normal', 'Recovers from short indexer outages.'],
			[10, 'Stubborn', 'Keeps trying a busy indexer.']] },
		UrlTimeout: { min: 5, max: 300, presets: [
			[30, 'Quick', 'Gives up on slow indexers.'],
			[60, 'Normal', ''],
			[180, 'Patient', 'For slow indexers.']] },
		ParBuffer: { min: 16, max: 4096, scale: 'sqrt', presets: [
			[100, 'Low', 'For machines with little memory; repair is slower and most releases repair only after the download.'],
			[500, 'Normal', 'Repairs about 96% of releases during the download (par2 blocks up to 28 MB).'],
			[2000, 'High', 'Faster repair of large releases; uses up to 2 GB.']] },
		ParThreads: { min: 0, max: 64, presets: [
			[0, 'Automatic', 'One thread per CPU core.'],
			[2, 'Background', 'Leaves the machine responsive during repair.'],
			[16, 'Many', 'Only on machines with many cores.']] },
		ParTimeLimit: { min: 0, max: 600, scale: 'sqrt', presets: [
			[0, 'No limit', 'Repair always runs to the end.'],
			[30, 'Short', 'Gives up on long repairs; the download is marked as failed.'],
			[120, 'Long', 'Allows big repairs to finish.']] },
		LogRotations: { min: 0, max: 1000, scale: 'sqrt', presets: [
			[0, 'Unlimited', 'Rolled-over logs are never deleted.'],
			[10, 'Normal', 'About 100 MB of log before compression.'],
			[100, 'Long', 'Keeps a long history.']] },
		LogBuffer: { min: 0, max: 100000, scale: 'sqrt', presets: [
			[0, 'Unlimited', 'Keeps every message until RustyGet restarts; memory use grows with it.'],
			[100, 'Small', 'The Messages tab shows only recent lines.'],
			[1000, 'Normal', ''],
			[20000, 'Large', 'Keeps more history in memory.']] },
		NzbDirInterval: { min: 0, max: 600, scale: 'sqrt', presets: [
			[0, 'Off', 'The NZB folder is not watched.'],
			[5, 'Quick', 'New files are picked up within seconds.'],
			[60, 'Relaxed', 'Checks once a minute.']] },
		NzbDirFileAge: { min: 0, max: 600, scale: 'sqrt', presets: [
			[10, 'Quick', 'May pick up a file that is still being written.'],
			[60, 'Normal', ''],
			[300, 'Safe', 'For slow network copies.']] },
		DupeStreamTimeout: { min: 0, max: 600, scale: 'sqrt', presets: [
			[0, 'No limit', 'A stream repair runs until it ends.'],
			[60, 'Normal', 'Stops a repair that recovers nothing for a minute.'],
			[300, 'Patient', 'For slow donors.']] },
		DupeCheckConcurrency: { min: 1, max: 50, presets: [
			[5, 'Gentle', 'Fewer copies are sampled at once.'],
			[20, 'Normal', ''],
			[50, 'Fast', 'Samples big fleets quickly; uses more connections.']] },
		DupeHealthBudget: { min: 10, max: 600, scale: 'sqrt', presets: [
			[30, 'Quick', 'Less sampling per copy; picks may be less accurate.'],
			[120, 'Normal', ''],
			[300, 'Thorough', 'More accurate picks for large fleets.']] },
		QuotaStartDay: { min: 1, max: 31 }
	};

	// numbers that may be negative: a feed's priority (-100 very low) and the clock offset
	var SIGNED_NUMBERS = ['Feed.Priority', 'TimeCorrection'];

	// fields typed as expressions, not stepped as one number: weekday lists and ranges
	// such as "1-5, 7", and times such as "08:00, 20:00"
	var EXPRESSION_FIELDS = ['Task.WeekDays', 'Task.Time'];

	// a range written in the option's help, such as (0-99)
	function numberMeta(option)
	{
		var meta = NUMBER_META[optionFamily(option.name)] || NUMBER_META[option.name];
		var unit = option.select && option.select[0];
		var range = /^\s*(-?[0-9]+)\s*-\s*(-?[0-9]+)\s*$/.exec(unit || '');
		if (!meta && range && !/^0/.test(range[1].substr(1)) && (+range[2] - +range[1]) <= 100)
		{
			meta = { min: +range[1], max: +range[2] };
		}
		if (meta)
		{
			// the slider spans the useful values; the box takes whatever the option allows
			meta = $.extend({}, meta, range ? { hardMin: +range[1], hardMax: +range[2] } : { hardMin: Math.min(0, meta.min) });
		}
		return meta;
	}

	function toScale(meta, value)
	{
		var span = meta.max - meta.min;
		var x = Math.max(0, Math.min(1, (value - meta.min) / (span || 1)));
		return meta.scale === 'sqrt' ? Math.sqrt(x) : x;
	}

	function fromScale(meta, x)
	{
		var v = meta.scale === 'sqrt' ? x * x : x;
		return Math.round(meta.min + v * (meta.max - meta.min));
	}

	function presetName(option, preset)
	{
		var key = 'config_preset_' + optionFamily(option.name).replace(/\./g, '_').toLowerCase() + '_' + preset[0];
		return I18n.defaultValue(key, preset[1]);
	}

	function presetText(option, preset)
	{
		var key = 'config_preset_' + optionFamily(option.name).replace(/\./g, '_').toLowerCase() + '_' + preset[0] + '_text';
		return preset[2] ? I18n.defaultValue(key, preset[2]) : '';
	}

	function setNumber(input, value, meta)
	{
		var n = parseInt(value, 10);
		if (isNaN(n))
		{
			n = meta ? meta.min : 0;
		}
		n = !meta && $(input).attr('pattern') === '-?[0-9]*' ? n : clampNumber(n, meta);
		$(input).val(String(n)).trigger('input').trigger('change');
	}

	function clampNumber(n, meta)
	{
		if (!meta)
		{
			return Math.max(0, n);
		}
		n = Math.max(meta.hardMin, n);
		return meta.hardMax === undefined ? n : Math.min(meta.hardMax, n);
	}

	function syncSlider(wrapper)
	{
		var input = wrapper.find('input.editnumeric');
		var meta = wrapper.data('meta');
		var range = wrapper.find('input[type="range"]');
		if (!meta || !range.length)
		{
			return;
		}
		var value = parseInt(input.val(), 10);
		range.val(Math.round(toScale(meta, isNaN(value) ? meta.min : value) * 1000));
		var onName = '';
		wrapper.find('.config-slider__dot').each(function()
		{
			var on = +$(this).attr('data-value') === value;
			$(this).toggleClass('is-on', on);
			if (on)
			{
				onName = $(this).attr('data-name');
			}
		});
		wrapper.find('.config-number__preset').text(onName || '');
	}

	function enhanceNumber(option, input)
	{
		var meta = numberMeta(option);
		var appendBox = input.closest('.input-append');
		var wrapper = $('<div class="config-number"></div>').data('meta', meta);
		appendBox.before(wrapper);
		wrapper.append(appendBox);
		input.attr({ inputmode: 'numeric', autocomplete: 'off', spellcheck: 'false' });
		var allowMinus = !!(meta && meta.min < 0) || SIGNED_NUMBERS.indexOf(optionFamily(option.name)) > -1;
		input.attr('pattern', allowMinus ? '-?[0-9]*' : '[0-9]*');
		var label = option.label || option.caption;
		var minus = $('<button type="button" class="btn config-step" data-step="-1">−</button>')
			.attr('aria-label', I18n.defaultValue('config_step_down', 'Decrease') + ' ' + label);
		var plus = $('<button type="button" class="btn config-step" data-step="1">+</button>')
			.attr('aria-label', I18n.defaultValue('config_step_up', 'Increase') + ' ' + label);
		input.before(minus).after(plus);
		if (meta && meta.max !== undefined)
		{
			var slider = $('<div class="config-slider"></div>');
			var range = $('<input type="range" min="0" max="1000" step="1">')
				.attr('aria-label', label);
			slider.append(range);
			var dots = $('<div class="config-slider__dots"></div>');
			(meta.presets || []).forEach(function(preset)
			{
				dots.append($('<button type="button" class="config-slider__dot"></button>')
					.attr('data-value', preset[0])
					.attr('data-name', presetName(option, preset))
					.attr('title', presetName(option, preset) + ' (' + preset[0] + ')')
					.attr('aria-label', presetName(option, preset) + ': ' + preset[0])
					.css('left', (toScale(meta, preset[0]) * 100) + '%'));
			});
			slider.append(dots);
			wrapper.prepend(slider);
			wrapper.append('<span class="config-number__preset" aria-hidden="true"></span>');
			range.on('input', function()
			{
				var value = fromScale(meta, this.value / 1000);
				// sticky presets: a value close to a dot lands on it
				(meta.presets || []).forEach(function(preset)
				{
					if (Math.abs(toScale(meta, preset[0]) - this.value / 1000) <= 0.025)
					{
						value = preset[0];
					}
				}, this);
				input.val(String(value)).trigger('input');
				syncSlider(wrapper);
			}).on('change', function() { input.trigger('change'); })
			.on('keydown', function(e)
			{
				// arrows step the setting by one, Page keys jump between presets, Home/End to the ends
				var value = parseInt(input.val(), 10);
				value = isNaN(value) ? meta.min : value;
				var presets = (meta.presets || []).map(function(p) { return p[0]; });
				var next = null;
				if (e.key === 'ArrowRight' || e.key === 'ArrowUp') next = value + 1;
				else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') next = value - 1;
				else if (e.key === 'Home') next = meta.min;
				else if (e.key === 'End') next = meta.max;
				else if (e.key === 'PageUp') next = presets.filter(function(p) { return p > value; })[0];
				else if (e.key === 'PageDown') next = presets.filter(function(p) { return p < value; }).pop();
				else return;
				e.preventDefault();
				if (next !== undefined && next !== null)
				{
					setNumber(input, next, meta);
					syncSlider(wrapper);
				}
			});
			dots.on('click', '.config-slider__dot', function()
			{
				setNumber(input, $(this).attr('data-value'), meta);
				syncSlider(wrapper);
			});
		}
		syncSlider(wrapper);
		// the help lists the presets with what each one means
		if (meta && meta.presets && meta.presets.length)
		{
			var help = input.closest('.control-group').find('.help-block').first();
			var table = $('<table class="config-presets"></table>');
			table.append($('<caption></caption>').text(I18n.defaultValue('config_presets', 'Presets')));
			meta.presets.forEach(function(preset)
			{
				table.append($('<tr></tr>')
					.append($('<th scope="row"></th>').text(presetName(option, preset)))
					.append($('<td class="config-presets__value"></td>').text(preset[0]))
					.append($('<td></td>').text(presetText(option, preset))));
			});
			if (help.length)
			{
				help.prepend(table);
			}
		}
	}

	function enhanceSelect(group)
	{
		var select = $('<select class="config-select"></select>');
		group.find('.btn').each(function()
		{
			var text = $(this).val();
			if (text === $(this).attr('data-value'))
			{
				text = text.charAt(0).toUpperCase() + text.substr(1);
			}
			select.append($('<option></option>').attr('value', $(this).attr('data-value')).text(text));
		});
		var label = group.closest('.control-group').find('.config-label').first().text() ||
			group.closest('.control-group').find('.option-name').first().text();
		select.attr('aria-label', label);
		select.val(switchGetValue(group));
		group.addClass('config-select-source').attr('aria-hidden', 'true').find('.btn').attr('tabindex', '-1');
		group.before(select);
		select.on('change', function()
		{
			var value = this.value;
			var btn = group.find('.btn').filter(function() { return $(this).attr('data-value') === value; }).first();
			if (btn.length)
			{
				Config.switchClick(btn[0]);
			}
		});
	}

	/*** PAR PRESET: five one-click resource levels ***************************/

	// ParPreset stays a row of buttons (not a menu). Below it a line says what the
	// chosen level sets on this machine; "detect" also shows what was measured and
	// can measure again. The values come from the daemon (parpresetdetect), which
	// knows the CPU count, RAM and the intermediate folder's disk.
	var parPresetInfo = null;
	function pt(key, text) { return I18n.defaultValue(key, text); }
	var parPresetProbeAsked = false;

	function enhanceParPreset(option, group)
	{
		if (group.hasClass('config-preset-buttons'))
		{
			return;
		}
		group.addClass('config-preset-buttons');
		var summary = $('<p class="config-preset-summary" aria-live="polite"></p>');
		var again = $('<button type="button" class="btn btn-default config-preset-detect"></button>')
			.text(pt('config_parpreset_detect_again', 'Measure again'))
			.on('click', function() { loadParPresetInfo(true); });
		group.after($('<div class="config-preset-info"></div>').append(summary).append(again));
		option.onchange = function() { renderParPresetSummary(); };
		loadParPresetInfo(false);
	}

	function loadParPresetInfo(start)
	{
		RPC.call('parpresetdetect', [!!start], function(data)
		{
			try
			{
				parPresetInfo = typeof data === 'string' ? JSON.parse(data) : data;
			}
			catch (e)
			{
				parPresetInfo = null;
			}
			renderParPresetSummary();
			if (parPresetInfo && parPresetInfo.Running)
			{
				setTimeout(function() { loadParPresetInfo(false); }, 1500);
			}
		}, function() { parPresetInfo = null; renderParPresetSummary(); });
	}

	function renderParPresetSummary()
	{
		var group = $('.config-preset-buttons', $ConfigData);
		if (!group.length)
		{
			return;
		}
		var preset = String(group.find('.btn-primary').attr('data-value') || 'custom').toLowerCase();
		var info = group.siblings('.config-preset-info');
		var summary = info.find('.config-preset-summary');
		info.find('.config-preset-detect').toggle(preset === 'detect');
		if (preset === 'custom')
		{
			summary.text(pt('config_parpreset_custom', 'Uses Par buffer, Par threads and Pause queue under Custom resources.'));
			return;
		}
		var values = parPresetInfo && parPresetInfo.Presets && parPresetInfo.Presets[preset];
		if (!values)
		{
			summary.text('');
			return;
		}
		var threads = values.ParThreads === 0 ? pt('config_parpreset_all_cpus', 'all CPUs') :
			values.ParThreads + ' ' + pt('config_parpreset_threads', 'threads');
		var text = values.ParBuffer + ' MB, ' + threads + ', ' +
			(values.ParPauseQueue ? pt('config_parpreset_pause', 'downloads pause while repairing') :
				pt('config_parpreset_nopause', 'downloads keep going while repairing'));
		if (preset === 'detect' && !parPresetInfo.Running && parPresetInfo.DiskWriteMBps === undefined && !parPresetProbeAsked)
		{
			// picking Detect measures this machine once, if it was not measured yet
			parPresetProbeAsked = true;
			loadParPresetInfo(true);
		}
		if (preset === 'detect')
		{
			var p = parPresetInfo;
			var seen = p.CPUs + ' CPUs';
			if (p.MemAvailableMB !== undefined)
			{
				seen += ', ' + p.MemAvailableMB + ' of ' + p.MemTotalMB + ' MB RAM free';
			}
			if (p.DiskWriteMBps !== undefined)
			{
				seen += ', disk ' + p.DiskWriteMBps + ' MB/s write' +
					(p.DiskReadMBps !== undefined ? ', ' + p.DiskReadMBps + ' MB/s read' : '');
			}
			else if (p.Running)
			{
				seen += ', ' + pt('config_parpreset_testing', 'testing the disk…');
			}
			text = seen + ' → ' + text;
			if (p.AppliedParBuffer !== undefined && p.AppliedParBuffer !== values.ParBuffer)
			{
				text += ' ' + pt('config_parpreset_applied', '(%s MB in use until settings are saved or reloaded)')
					.replace('%s', p.AppliedParBuffer);
			}
		}
		summary.text(text);
	}

	// numbers get a stepper (and a slider when they have limits), short choices a menu
	function enhanceControls(scope)
	{
		scope = scope || $ConfigData;
		allOptions(function(option)
		{
			var control = $(document.getElementById(option.formId));
			if (!control.length || (scope[0] !== $ConfigData[0] && !$.contains(scope[0], control[0])))
			{
				return;
			}
			// times (HH:MM) and octal masks (000-1000) are typed as written, not stepped
			var unit = String(option.select && option.select[0] || '');
			if (option.type === 'numeric' && control.is('input.editnumeric') && !control.closest('.config-number').length &&
				!/:|^\s*0[0-9]/.test(unit) && EXPRESSION_FIELDS.indexOf(optionFamily(option.name)) === -1)
			{
				enhanceNumber(option, control);
			}
			else if (option.name === 'ParPreset' && control.is('.btn-switch'))
			{
				enhanceParPreset(option, control);
			}
			else if (option.type === 'switch' && control.is('.btn-switch') && !control.hasClass('is-toggle') &&
				!control.hasClass('config-select-source'))
			{
				enhanceSelect(control);
			}
			else if (optionFamily(option.name) === 'Server.Cipher' && control.is('input') && !control.hasClass('config-cipher-source'))
			{
				enhanceCipher(option, control);
			}
			else if (optionFamily(option.name) === 'Server.Host' && control.is('input'))
			{
				enhanceHost(option, control);
			}
			else if (optionFamily(option.name) === 'Server.Backbone' && control.is('input') && !control.hasClass('config-backbone-source'))
			{
				enhanceBackbone(option, control);
			}
			if (optionFamily(option.name) === 'Server.Port' && control.is('input'))
			{
				enhanceHostPort(control);
			}
		});
	}
	this.enhanceControls = enhanceControls;

	// keep menus and sliders in step with values set from elsewhere (Discard, restore, dialogs)
	function syncControls()
	{
		$('select.config-cipher__menu', $ConfigData).each(function() { syncCipher($(this)); });
		$('input.config-backbone-source', $ConfigData).each(function() { syncBackbone($(this)); });
		$('select.config-select', $ConfigData).not('.config-cipher__menu').not('.config-backbone__menu').each(function()
		{
			var group = $(this).next('.btn-switch');
			var value = switchGetValue(group);
			if (this.value !== value)
			{
				if (!$(this).find('option').filter(function() { return this.value === value; }).length)
				{
					$(this).append($('<option></option>').attr('value', value).text(value));
				}
				$(this).val(value);
			}
		});
		$('.config-number', $ConfigData).each(function() { syncSlider($(this)); });
	}

	// Renaming: one menu for the three rename options. Expert still shows each one.
	var RENAMING = {
		during: { DirectRename: 'yes', ParRename: 'yes', RarRename: 'yes' },
		after: { DirectRename: 'no', ParRename: 'yes', RarRename: 'yes' },
		off: { DirectRename: 'no', ParRename: 'no', RarRename: 'no' }
	};
	var RENAMING_LABELS = { during: 'While downloading', after: 'After download', off: 'Off', custom: 'Custom (see Expert)' };

	function renamingValue()
	{
		var current = {};
		['DirectRename', 'ParRename', 'RarRename'].forEach(function(name)
		{
			var option = findOptionByName(name);
			current[name] = option ? currentValue(option).toLowerCase() : '';
		});
		for (var key in RENAMING)
		{
			if (Object.keys(RENAMING[key]).every(function(name) { return current[name] === RENAMING[key][name]; }))
			{
				return key;
			}
		}
		return 'custom';
	}

	function addRenamingControl()
	{
		var anchor = $('#S_DirectRename').closest('.control-group');
		if (!anchor.length || $('#ConfigRenaming').length)
		{
			return;
		}
		var t = function(key, text) { return I18n.defaultValue(key, text); };
		var row = $('<div class="control-group config-option-tile config-virtual in-card" data-virtual-mode="advanced">' +
			'<label class="control-label"><span class="option-name"><span class="config-label"></span></span></label>' +
			'<div class="controls"><select id="ConfigRenaming" class="config-select"></select></div>' +
			'<p class="config-option-summary"></p></div>').addClass(anchor.attr('class').match(/\bCHECK_AND_REPAIR\b/) ? 'CHECK_AND_REPAIR' : '');
		row.find('.config-label').text(t('config_renaming', 'Renaming'));
		row.find('.config-option-summary').text(t('config_renaming_text', 'When to restore the original file names from par2 and RAR files.'));
		var select = row.find('select').attr('aria-label', t('config_renaming', 'Renaming'));
		['during', 'after', 'off', 'custom'].forEach(function(key)
		{
			select.append($('<option></option>').attr('value', key).text(t('config_renaming_' + key, RENAMING_LABELS[key]))
				.prop('disabled', key === 'custom'));
		});
		select.on('change', function()
		{
			var values = RENAMING[this.value];
			if (!values)
			{
				return;
			}
			Object.keys(values).forEach(function(name)
			{
				var option = findOptionByName(name);
				if (option && option.formId)
				{
					setOptionValue(option, values[name]);
					$('#' + option.formId).attr('aria-checked', values[name] === 'yes' ? 'true' : 'false');
				}
			});
			refreshChangeState();
		});
		anchor.before(row);
		syncRenaming();
	}

	function syncRenaming()
	{
		var select = $('#ConfigRenaming');
		if (select.length)
		{
			select.val(renamingValue());
		}
	}

	// Listen address: a checkbox for "other computers too" (0.0.0.0) beside the field
	function addListenAllControl()
	{
		var input = $('#S_ControlIP');
		if (!input.length || $('#ConfigListenAll').length)
		{
			return;
		}
		var label = $('<label class="config-inline-check"><input type="checkbox" id="ConfigListenAll"> <span></span></label>');
		label.find('span').text(I18n.defaultValue('config_listen_all', 'Allow other computers (listen on all interfaces, 0.0.0.0)'));
		var wrap = $('<div class="config-listen"></div>');
		input.before(wrap);
		wrap.append(input).append(label);
		var box = label.find('input');
		box.on('change', function()
		{
			input.val(this.checked ? '0.0.0.0' : '127.0.0.1').trigger('input').trigger('change');
		});
		syncListenAll();
	}

	function syncListenAll()
	{
		var input = $('#S_ControlIP');
		$('#ConfigListenAll').prop('checked', $.trim(input.val()) === '0.0.0.0');
	}

	/*** HTTPS AND CERTIFICATES *******************************************************/

	// HTTPS shares the web port, so the card only asks for a certificate and its key,
	// and only while "Use HTTPS" is on. The two paths get a Browse button (a browser
	// of the daemon's own files), and one row under them makes a self-signed
	// certificate, saves a pasted certificate and key, and checks that the pair fits.
	// The CA store for checking news servers gets Browse, Upload, Paste and Check.
	function certText(key, text)
	{
		return I18n.defaultValue('config_cert_' + key, text);
	}

	function certButton(key, text)
	{
		return $('<button type="button" class="btn btn-default config-cert-btn"></button>').text(certText(key, text));
	}

	function addBrowseButton(name, title)
	{
		var option = findOptionByName(name);
		var input = option && $('#' + option.formId);
		if (!input || !input.length || input.siblings('.config-cert-browse').length)
		{
			return;
		}
		certButton('browse', 'Browse…').addClass('config-cert-browse').attr('aria-label', certText('browse', 'Browse…') + ' ' + title)
			.insertAfter(input).on('click', function()
			{
				FileBrowserDialog.showModal(title, $.trim(input.val()) || '${MainDir}', function(path)
				{
					input.val(path).trigger('input').trigger('change');
				});
			});
	}

	function setPathOption(name, value)
	{
		var option = findOptionByName(name);
		if (option && value)
		{
			$('#' + option.formId).val(value).trigger('input').trigger('change');
		}
	}

	function certStatus(box, state, lines)
	{
		box.attr('data-state', state).empty();
		lines.forEach(function(line)
		{
			box.append($('<p></p>').text(line));
		});
		box.prop('hidden', !lines.length);
	}

	function describeCert(check)
	{
		if (!check || check.Error)
		{
			return { state: 'error', lines: [check ? check.Error : certText('none', 'No certificate.')] };
		}
		var lines = [];
		var names = (check.Names || []).join(', ') || check.Subject;
		lines.push(certText('for', 'For') + ' ' + names + (check.SelfSigned ? ' (' + certText('self_signed', 'self-signed') + ')' : ''));
		lines.push((check.Expired ? certText('expired', 'Expired') : certText('expires', 'Expires')) + ' ' + check.NotAfter);
		if (check.NotYetValid)
		{
			lines.push(certText('not_yet', 'Not valid yet.'));
		}
		lines.push(check.KeyMatches === true ? certText('key_ok', 'The key belongs to this certificate.') :
			check.KeyMatches === false ? certText('key_bad', 'The key does not belong to this certificate.') :
			certText('key_none', 'No key file is set.'));
		return { state: check.Ok && check.KeyMatches === true ? 'ok' : 'error', lines: lines };
	}

	var httpsCheckTimer;
	var httpsCheckSeq = 0;

	function checkHttpsPair()
	{
		var box = $('#ConfigHttpsStatus');
		var cert = findOptionByName('SecureCert'), key = findOptionByName('SecureKey');
		if (!box.length || !cert || !key)
		{
			return;
		}
		var certPath = $.trim(getOptionValue(cert) || ''), keyPath = $.trim(getOptionValue(key) || '');
		var seq = ++httpsCheckSeq;
		if (!certPath)
		{
			certStatus(box, 'error', [certText('need', 'HTTPS needs a certificate and its key. Make a self-signed one, paste yours, or browse to the files.')]);
			return;
		}
		certStatus(box, 'busy', [certText('checking', 'Checking…')]);
		RPC.call('checkcert', [certPath, keyPath], function(result)
		{
			if (seq === httpsCheckSeq)
			{
				var d = describeCert(result);
				certStatus(box, d.state, d.lines);
			}
		}, function(message)
		{
			if (seq === httpsCheckSeq)
			{
				certStatus(box, 'error', [String(message)]);
			}
		});
	}

	function scheduleHttpsCheck()
	{
		clearTimeout(httpsCheckTimer);
		httpsCheckTimer = setTimeout(checkHttpsPair, 500);
	}

	function addHttpsControls()
	{
		var keyOption = findOptionByName('SecureKey');
		var anchor = keyOption && $('#' + keyOption.formId).closest('.control-group');
		if (!anchor || !anchor.length || $('#ConfigHttpsTools').length)
		{
			return;
		}
		addBrowseButton('SecureCert', certText('cert_title', 'HTTPS certificate'));
		addBrowseButton('SecureKey', certText('key_title', 'HTTPS private key'));
		var section = (anchor.attr('class').match(/\bSECURITY\b/) || [''])[0];
		var row = $('<div class="control-group config-option-tile config-virtual in-card config-https-tools" id="ConfigHttpsTools">' +
			'<label class="control-label"><span class="option-name"><span class="config-label"></span></span></label>' +
			'<div class="controls"><div class="config-cert-actions"></div>' +
			'<div class="config-cert-status" id="ConfigHttpsStatus" aria-live="polite" hidden></div>' +
			'<div class="config-cert-form" id="ConfigHttpsMake" hidden>' +
				'<label for="ConfigHttpsNames"></label><input type="text" id="ConfigHttpsNames" class="config-cert-names" spellcheck="false">' +
				'<p class="config-cert-hint"></p><div class="config-cert-form-actions"></div></div>' +
			'<div class="config-cert-form" id="ConfigHttpsPaste" hidden>' +
				'<label for="ConfigHttpsPem"></label><textarea id="ConfigHttpsPem" rows="8" spellcheck="false"></textarea>' +
				'<div class="config-cert-form-actions"></div></div>' +
			'</div></div>').addClass(section);
		row.find('.config-label').text(certText('pair', 'Certificate and key'));
		var actions = row.find('.config-cert-actions');
		var make = row.find('#ConfigHttpsMake'), paste = row.find('#ConfigHttpsPaste');
		var toggle = function(form)
		{
			var open = form.prop('hidden');
			make.prop('hidden', true);
			paste.prop('hidden', true);
			form.prop('hidden', !open);
			if (open)
			{
				form.find('input, textarea').first().focus();
			}
		};
		actions.append(certButton('make', 'Make a self-signed certificate').attr('id', 'ConfigHttpsMakeBtn').on('click', function()
		{
			var names = $('#ConfigHttpsNames');
			if (!$.trim(names.val()))
			{
				names.val(location.hostname || 'localhost');
			}
			toggle(make);
		}));
		actions.append(certButton('paste', 'Paste certificate and key').attr('id', 'ConfigHttpsPasteBtn').on('click', function() { toggle(paste); }));
		actions.append(certButton('check', 'Check').attr('id', 'ConfigHttpsCheckBtn').on('click', checkHttpsPair));

		make.find('label').text(certText('names', 'Names the certificate is for'));
		make.find('.config-cert-hint').text(certText('names_hint',
			'The name or address you open RustyGet with. Add more, separated by commas, to use it from other names too.'));
		make.find('.config-cert-form-actions').append(certButton('make_go', 'Make certificate').addClass('btn-primary').attr('id', 'ConfigHttpsMakeGo')
			.on('click', function()
			{
				var button = $(this).prop('disabled', true);
				certStatus($('#ConfigHttpsStatus'), 'busy', [certText('making', 'Making a certificate…')]);
				RPC.call('makecert', [$('#ConfigHttpsNames').val()], function(result)
				{
					button.prop('disabled', false);
					httpsSaved(result);
					if (!result.Error)
					{
						make.prop('hidden', true);
					}
				}, function(message)
				{
					button.prop('disabled', false);
					certStatus($('#ConfigHttpsStatus'), 'error', [String(message)]);
				});
			}));
		paste.find('label').text(certText('pem', 'Certificate and private key, in PEM (the -----BEGIN lines included)'));
		paste.find('.config-cert-form-actions').append(certButton('save_pem', 'Use this').addClass('btn-primary').attr('id', 'ConfigHttpsPasteGo')
			.on('click', function()
			{
				var button = $(this).prop('disabled', true);
				RPC.call('savepem', ['https', '', $('#ConfigHttpsPem').val()], function(result)
				{
					button.prop('disabled', false);
					httpsSaved(result);
					if (!result.Error)
					{
						$('#ConfigHttpsPem').val('');
						paste.prop('hidden', true);
					}
				}, function(message)
				{
					button.prop('disabled', false);
					certStatus($('#ConfigHttpsStatus'), 'error', [String(message)]);
				});
			}));
		anchor.after(row);

		['SecureCert', 'SecureKey'].forEach(function(name)
		{
			var option = findOptionByName(name);
			$('#' + option.formId).on('input.https change.https', scheduleHttpsCheck);
		});
		syncHttps();
	}

	// A made or pasted pair was written by the daemon: point the options at it.
	function httpsSaved(result)
	{
		if (result.Error)
		{
			certStatus($('#ConfigHttpsStatus'), 'error', [result.Error]);
			return;
		}
		clearTimeout(httpsCheckTimer);
		++httpsCheckSeq;
		setPathOption('SecureCert', result.Cert);
		setPathOption('SecureKey', result.Key);
		clearTimeout(httpsCheckTimer);
		if (result.Key && result.Check)
		{
			var d = describeCert(result.Check);
			d.lines.push(certText('save_note', 'Save the settings to use it.'));
			certStatus($('#ConfigHttpsStatus'), d.state, d.lines);
		}
		else
		{
			checkHttpsPair();
		}
	}

	var httpsWasOn = null;

	function syncHttps()
	{
		var control = findOptionByName('SecureControl');
		if (!control || !$('#ConfigHttpsTools').length)
		{
			return;
		}
		var on = String(currentValue(control)).toLowerCase() === 'yes';
		['SecureCert', 'SecureKey'].forEach(function(name)
		{
			var option = findOptionByName(name);
			$('#' + option.formId).closest('.control-group').toggleClass('config-https-off', !on);
		});
		$('#ConfigHttpsTools').toggleClass('config-https-off', !on);
		if (on && httpsWasOn !== true)
		{
			checkHttpsPair();
		}
		httpsWasOn = on;
	}

	function addCaControls()
	{
		var option = findOptionByName('CertStore');
		var input = option && $('#' + option.formId);
		var anchor = input && input.closest('.control-group');
		if (!anchor || !anchor.length || $('#ConfigCaTools').length)
		{
			return;
		}
		addBrowseButton('CertStore', certText('ca_title', 'CA certificates'));
		var section = (anchor.attr('class').match(/\bSECURITY\b/) || [''])[0];
		var row = $('<div class="control-group config-option-tile config-virtual in-card config-ca-tools" id="ConfigCaTools">' +
			'<label class="control-label"><span class="option-name"><span class="config-label"></span></span></label>' +
			'<div class="controls"><div class="config-cert-actions"></div>' +
			'<input type="file" id="ConfigCaFile" accept=".pem,.crt,.cer,text/plain" hidden>' +
			'<div class="config-cert-status" id="ConfigCaStatus" aria-live="polite" hidden></div>' +
			'<div class="config-cert-form" id="ConfigCaPaste" hidden>' +
				'<label for="ConfigCaPem"></label><textarea id="ConfigCaPem" rows="6" spellcheck="false"></textarea>' +
				'<div class="config-cert-form-actions"></div></div>' +
			'</div></div>').addClass(section);
		row.find('.config-label').text(certText('ca_tools', 'Add or check CA certificates'));
		var actions = row.find('.config-cert-actions');
		var status = row.find('#ConfigCaStatus');
		var saveCa = function(name, text, done)
		{
			RPC.call('savepem', ['ca', name, text], function(result)
			{
				if (result.Error)
				{
					certStatus(status, 'error', [result.Error]);
					return;
				}
				setPathOption('CertStore', result.Path);
				certStatus(status, 'ok', [result.Count + ' ' + certText('ca_saved', 'certificates saved to') + ' ' + result.Path,
					certText('save_note', 'Save the settings to use it.')]);
				if (done) done();
			}, function(message) { certStatus(status, 'error', [String(message)]); });
		};
		actions.append(certButton('upload', 'Upload…').attr('id', 'ConfigCaUploadBtn').on('click', function() { $('#ConfigCaFile').click(); }));
		actions.append(certButton('paste_ca', 'Paste…').attr('id', 'ConfigCaPasteBtn').on('click', function()
		{
			var form = $('#ConfigCaPaste');
			form.prop('hidden', !form.prop('hidden'));
			if (!form.prop('hidden')) $('#ConfigCaPem').focus();
		}));
		actions.append(certButton('check', 'Check').attr('id', 'ConfigCaCheckBtn').on('click', function()
		{
			certStatus(status, 'busy', [certText('checking', 'Checking…')]);
			RPC.call('checkcastore', [$.trim(input.val())], function(result)
			{
				certStatus(status, result.Error ? 'error' : 'ok', [result.Error ? result.Error :
					result.Count + ' ' + certText('ca_count', 'certificates found')]);
			}, function(message) { certStatus(status, 'error', [String(message)]); });
		}));
		row.find('#ConfigCaFile').on('change', function()
		{
			var file = this.files && this.files[0];
			var picker = this;
			if (!file)
			{
				return;
			}
			if (file.size > 1048576)
			{
				certStatus(status, 'error', [certText('too_big', 'That file is too large to be a certificate.')]);
				return;
			}
			var reader = new FileReader();
			reader.onload = function() { saveCa(file.name, String(reader.result)); picker.value = ''; };
			reader.readAsText(file);
		});
		row.find('#ConfigCaPaste label').text(certText('ca_pem', 'CA certificates, in PEM (the -----BEGIN lines included)'));
		row.find('#ConfigCaPaste .config-cert-form-actions').append(certButton('save_pem', 'Use this').addClass('btn-primary')
			.attr('id', 'ConfigCaPasteGo').on('click', function()
			{
				saveCa('pasted', $('#ConfigCaPem').val(), function()
				{
					$('#ConfigCaPem').val('');
					$('#ConfigCaPaste').prop('hidden', true);
				});
			}));
		anchor.after(row);
	}

	// An option that only matters while another one has certain values is greyed out
	// otherwise, with a note that says what to change. Its value is kept and saved.
	var OPTION_REQUIRES = [
		{ option: 'DirectUnpack', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'UnpackCleanupDisk', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'UnpackPassFile', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'UseTempUnpackDir', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'UnpackPauseQueue', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'UnpackMaxFiles', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'UnpackMaxDepth', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'UnpackMaxFolders', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'ExtCleanupDisk', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'UnpackIgnoreExt', when: { name: 'Unpack', values: ['yes'], orCategory: 'Unpack' } },
		{ option: 'DupeBorrowFirst', when: { name: 'DupeArticleFallback', values: ['article', 'stream', 'live', 'yes'] } },
		{ option: 'DupeStreamDecompress', when: { name: 'DupeArticleFallback', values: ['stream', 'live'] } },
		{ option: 'DupeStreamTimeout', when: { name: 'DupeArticleFallback', values: ['stream', 'live'] } },
		{ option: 'DupeHealthPercent', when: { name: 'DupeHealthPreset', values: ['custom'] },
			text: ['config_requires_custom_sampling', 'Used only with "Health sampling" set to "custom".'] },
		{ option: 'DupeHealthMin', when: { name: 'DupeHealthPreset', values: ['custom'] },
			text: ['config_requires_custom_sampling', 'Used only with "Health sampling" set to "custom".'] },
		{ option: 'DupeHealthMax', when: { name: 'DupeHealthPreset', values: ['custom'] },
			text: ['config_requires_custom_sampling', 'Used only with "Health sampling" set to "custom".'] },
		{ option: 'DupeBodyChecks', when: { name: 'DupeHealthPreset', values: ['custom'] },
			text: ['config_requires_custom_sampling', 'Used only with "Health sampling" set to "custom".'] },
		{ option: 'DupeBodyPercent', when: { name: 'DupeHealthPreset', values: ['custom'] },
			text: ['config_requires_custom_sampling', 'Used only with "Health sampling" set to "custom".'] },
		{ option: 'DupeHealthBudget', when: { name: 'DupeHealthPreset', values: ['custom'] },
			text: ['config_requires_custom_sampling', 'Used only with "Health sampling" set to "custom".'] },
		{ option: 'HardLinkingIgnoreExt', when: { name: 'HardLinking', values: ['yes'] } },
		{ option: 'FlushQueue', when: { name: 'DirectWrite', values: ['no'] },
			text: ['config_requires_flush', 'Always on while "Write directly to the final file" is on: the queue file is how a download resumes.'] },
		{ option: 'HardLinking', when: { name: 'DirectRename', values: ['yes'] }, go: 'ConfigRenaming',
			text: ['config_requires_renaming', 'Set "Renaming" to "While downloading" to change this.'] },
		{ option: 'RestrictedPassword', when: { name: 'RestrictedUsername', notEmpty: true } },
		{ option: 'AddPassword', when: { name: 'AddUsername', notEmpty: true } }
	];

	function anyCategoryValue(field, values)
	{
		var found = false;
		allOptions(function(option)
		{
			if (!found && optionFamily(option.name) === 'Category.' + field &&
				values.indexOf(currentValue(option).toLowerCase()) > -1)
			{
				found = true;
			}
		});
		return found;
	}

	function currentValue(option)
	{
		var control = $(document.getElementById(option.formId));
		return control.length ? String(getOptionValue(option) || '') : String(option.value === null ? option.defvalue : option.value);
	}

	function choiceLabel(option, value)
	{
		var low = String(value).toLowerCase();
		if (low === 'yes' || low === 'no')
		{
			return I18n.defaultValue(low === 'yes' ? 'label_yes_cap' : 'label_no_cap', value);
		}
		return value;
	}

	function requirementText(rule, needed)
	{
		var name = needed.label || needed.caption;
		if (rule.when.notEmpty)
		{
			return I18n.defaultValue('config_requires_filled', 'Fill in "%1" first to change this.').replace('%1', name);
		}
		var values = rule.when.values.filter(function(v) { return v !== 'yes' || rule.when.values.length === 1; });
		if (values.length === 1 && values[0] === 'yes')
		{
			return I18n.defaultValue('config_requires_on', 'Turn on "%1" to change this.').replace('%1', name);
		}
		return I18n.defaultValue('config_requires_value', 'Set "%1" to %2 to change this.').replace('%1', name)
			.replace('%2', values.map(function(v) { return choiceLabel(needed, v); }).join(I18n.defaultValue('config_requires_or', ' or ')));
	}

	function setLocked(row, locked, text, neededId, anchorId)
	{
		row.toggleClass('is-locked', locked);
		var controls = row.children('.controls');
		controls.find('input, select, textarea, button').not('.config-help-toggle').prop('disabled', locked);
		controls.find('.btn-switch.is-toggle').attr('aria-disabled', locked ? 'true' : null).attr('tabindex', locked ? '-1' : '0');
		var note = row.children('.config-lock-note');
		if (!locked)
		{
			note.remove();
			return;
		}
		if (!note.length)
		{
			note = $('<div class="config-lock-note" role="note"><i class="material-icon" aria-hidden="true">info</i><span></span> <a href="#"></a></div>');
			row.append(note);
		}
		note.children('span').text(text);
		note.children('a').text(I18n.defaultValue('config_requires_go', 'Show me')).off('click').on('click', function(e)
		{
			e.preventDefault();
			if (findOptionById(neededId))
			{
				scrollOptionIntoView(neededId);
			}
			else
			{
				// a combined control such as Renaming is not an option: open its page first,
				// so the link also works from search results
				var target = document.getElementById(neededId);
				var anchor = anchorId && findOptionById(anchorId);
				if (anchor && ($ConfigContent.hasClass('search') || !$(target).is(':visible')))
				{
					Config.showSection(pageIdOf(anchor), false);
				}
				$(target).closest('details.config-card__more').prop('open', true);
				target.scrollIntoView({ block: 'center' });
			}
			$('#' + neededId).focus();
		});
	}

	function refreshRequirements()
	{
		if (!config)
		{
			return;
		}
		OPTION_REQUIRES.forEach(function(rule)
		{
			var target = findOptionByName(rule.option);
			var needed = findOptionByName(rule.when.name);
			if (!target || !needed || !target.formId || !needed.formId)
			{
				return;
			}
			var row = $(document.getElementById(target.formId)).closest('.control-group');
			if (!row.length)
			{
				return;
			}
			var value = currentValue(needed).toLowerCase();
			var met = rule.when.notEmpty ? value.trim() !== '' : rule.when.values.indexOf(value) > -1;
			// a category with Unpack=yes unpacks its downloads even while Unpack is off
			if (!met && rule.when.orCategory && anyCategoryValue(rule.when.orCategory, rule.when.values))
			{
				met = true;
			}
			var text = rule.text ? I18n.defaultValue(rule.text[0], rule.text[1]) : requirementText(rule, needed);
			var go = rule.go && document.getElementById(rule.go) && inMode('advanced') && !inMode('expert') ? rule.go : needed.formId;
			setLocked(row, !met, met ? '' : text, go, needed.formId);
		});
	}

	function refreshControls()
	{
		syncControls();
		syncRenaming();
		syncListenAll();
		syncHttps();
		refreshRequirements();
	}

	// Settings sections in task order, under group labels.
	var NAV_GROUPS = [
		{ key: 'config_group_start', label: 'Start here', ids: ['Config-Info', 'NEWS-SERVERS', 'PATHS', 'CATEGORIES'] },
		{ key: 'config_group_downloading', label: 'Downloading', ids: ['DOWNLOAD_QUEUE', 'DUPE_FLEETS', 'CONNECTION', 'CHECK_AND_REPAIR', 'UNPACK'] },
		{ key: 'config_group_automation', label: 'Automation', ids: ['RSS_FEEDS', 'INCOMING_NZBS', 'SCHEDULER'] },
		{ key: 'config_group_system', label: 'System', ids: ['SECURITY', 'WEB-INTERFACE', 'LOGGING', 'Config-System', 'Config-SystemInfo', 'Config-Licenses'] }
	];

	function groupNav()
	{
		var items = {};
		var extensions = [];
		$ConfigNav.children('li.config-nav-group, li.divider').not('.ConfigSearch').remove();
		$ConfigNav.children('li').not('.ConfigSearch').each(function()
		{
			var id = ($('> a', this).attr('href') || '').substr(1);
			items[id] = this;
			var known = NAV_GROUPS.some(function(group) { return group.ids.indexOf(id) > -1; });
			if (!known)
			{
				extensions.push(this);
			}
		});
		var search = $ConfigNav.children('li.ConfigSearch').detach();
		NAV_GROUPS.forEach(function(group)
		{
			var members = group.ids.map(function(id) { return items[id]; }).filter(Boolean);
			if (group.key === 'config_group_system')
			{
				// anything not listed above (nothing today) stays reachable at the end
				members = extensions.concat(members);
			}
			if (members.length === 0)
			{
				return;
			}
			$ConfigNav.append($('<li class="config-nav-group nav-header"></li>').text(I18n.defaultValue(group.key, group.label)));
			members.forEach(function(item)
			{
				$(item).removeClass('list-item--nested');
				$(item).find('a, a > span').contents().filter(function() { return this.nodeType === 3 && $.trim(this.nodeValue) !== ''; })
					.each(function() { this.nodeValue = sentenceCase($.trim(this.nodeValue)); });
				$ConfigNav.append(item);
			});
		});
		$ConfigNav.append(search);
	}

	// What each control showed when the page was built: the saved value, or the default
	// for an option the configuration file doesn't have yet.
	function snapshotShownValues()
	{
		eachEditableOption(function(option)
		{
			option.shownValue = getOptionValue(option);
		});
	}

	function eachEditableOption(callback)
	{
		for (var k=0; k < config.length; k++)
		{
			var sections = config[k].sections;
			for (var i=0; i < sections.length; i++)
			{
				var section = sections[i];
				if (section.postparam || section.hidden)
				{
					continue;
				}
				for (var j=0; j < section.options.length; j++)
				{
					var option = section.options[j];
					if (!option.template && option.type !== 'info' && !option.commandopts && option.formId)
					{
						callback(option, section);
					}
				}
			}
		}
	}

	// Options whose control differs from the saved value.
	function changedOptions()
	{
		var changed = [];
		var listChanged = false;
		if (!config)
		{
			return { options: changed, lists: false, restored: false };
		}
		for (var k=0; k < config.length; k++)
		{
			config[k].sections.forEach(function(section)
			{
				listChanged = listChanged || (!section.postparam && !section.hidden && !!section.modified);
			});
		}
		eachEditableOption(function(option)
		{
			var baseline = option.value !== null ? option.value : option.shownValue;
			if (baseline === undefined || baseline === null)
			{
				return; // a set added since the page was built: counted as a list change
			}
			var current = getOptionValue(option);
			if (current != null && !isSameOptionValue(option, baseline, current))
			{
				changed.push(option);
			}
		});
		return { options: changed, lists: listChanged, restored: restored };
	}

	/*** INLINE VALIDATION *******************************************************/

	// Plain words first, for options whose built-in description is technical.
	var PLAIN_HELP = {
		'maindir': 'The folder RustyGet works in. The other folders are inside it unless you choose otherwise. ~ means your home folder.',
		'destdir': 'Where finished downloads end up.',
		'interdir': 'Where downloads are kept while they are not finished yet. Leave it empty to download straight into the finished-downloads folder.',
		'nzbdir': 'Drop .nzb files into this folder and RustyGet picks them up by itself.',
		'queuedir': 'Where RustyGet remembers the queue and history. Do not delete it.',
		'tempdir': 'Scratch space for temporary files.',
		'webdir': 'Leave this empty: RustyGet then uses the web interface built into it.',
		'configtemplate': 'Leave this empty: the option descriptions built into RustyGet are used.',
		'controlip': 'Which network address the web interface listens on. 127.0.0.1 means this computer only; 0.0.0.0 means every computer that can reach this one.',
		'controlport': 'The port in the web interface address (http://address:port/). 1 to 65535; 6789 is the usual one.',
		'controlusername': 'The user name you sign in to the web interface with.',
		'controlpassword': 'The password you sign in to the web interface with. Always set one.',
		'formauth': 'Yes shows a sign-in page; No uses the browser’s own sign-in box.',
		'server1.host': 'The address of your news server, for example news.example.com (no http://).',
		'server1.port': 'Usually 563 with encryption and 119 without. Your provider tells you.',
		'server1.encryption': 'Encrypt the connection to the news server. Keep it on unless your provider does not support it.',
		'server1.connections': 'How many downloads run at once on this server. Your provider says how many you may use.',
		'server1.username': 'The user name of your Usenet account.',
		'server1.password': 'The password of your Usenet account.',
		'server1.active': 'Turn the server off to stop using it without deleting it.',
		'server1.level': 'Servers with level 0 are used first; higher levels are backups for missing articles.',
		'articlecache': 'Memory used to hold downloaded pieces before writing them. More is faster; 0 turns it off.',
		'downloadrate': 'Speed limit in kilobytes per second. 0 means no limit.',
		'diskspace': 'Downloading pauses when less free space than this (in megabytes) is left.',
		'keephistory': 'How many days finished items stay in History. 0 removes them at once.',
		'umask': 'File permissions for new files, as an octal number. 1000 keeps the system default.',
		'parcheck': 'When to check downloads with their repair files. "auto" checks only when something is damaged.',
		'healthcheck': 'What to do when a download cannot be completed because too much is missing.',
		'dupecheck': 'Skip items you already downloaded.',
		'securecontrol': 'Also answer HTTPS on the web interface port.'
	};

	function familyName(name)
	{
		return String(name || '').toLowerCase().replace(/^([a-z]+)[0-9]+\./, '$11.');
	}

	function plainHelp(option)
	{
		return option && option.name && option.name.indexOf(':') === -1 ? PLAIN_HELP[familyName(option.name)] || '' : '';
	}

	// other words RustyGet takes for a choice (as options.rs reads them)
	var CHOICE_ALIASES = {
		'dupearticlefallback': ['yes'],
		'outputmode': ['loggable', 'logable', 'log', 'colored', 'color', 'ncurses', 'curses']
	};
	var REQUIRED_DIRS = ['maindir', 'destdir', 'tempdir', 'queuedir', 'nzbdir'];
	var CHECKED_DIRS = ['maindir', 'destdir', 'interdir', 'tempdir', 'queuedir', 'nzbdir', 'category1.destdir'];
	var HOST_RE = /^(\[[0-9a-fA-F:.%]+\]|[A-Za-z0-9_][A-Za-z0-9._-]*|[0-9a-fA-F:.%]+)$/;
	var pathChecks = {}; // "value\u0001maindir" -> {state, message, path} or 'pending'
	var validationTimer = null;
	var invalidOptions = [];

	function numberRule(option)
	{
		if (!option.select || option.select.length > 1 || !/^-?[0-9]+$/.test($.trim(option.defvalue || '')))
		{
			return null;
		}
		var m = /^\s*(-?[0-9]+)\s*-\s*(-?[0-9]+)\s*$/.exec(option.select[0] || '');
		if (m && !(m[1].length > 1 && m[1][0] === '0'))
		{
			return { min: +m[1], max: +m[2] };
		}
		return { min: null, max: null };
	}

	function serverActive(name)
	{
		var m = /^(Server[0-9]+)\./i.exec(name);
		if (!m) return true;
		var active = findOptionByName(m[1] + '.Active');
		return !active || String(getOptionValue(active) || 'yes').toLowerCase() !== 'no';
	}

	// The problem with this value, in plain words, or '' (folders: the last check).
	function optionProblem(option, value)
	{
		var family = familyName(option.name);
		value = value == null ? '' : String(value);
		var trimmed = $.trim(value);
		if (/[\r\n]/.test(value))
		{
			return I18n.defaultValue('config_invalid_newline', 'Enter it on one line.');
		}
		var rule = numberRule(option);
		if (rule && !/^\$\{/.test(trimmed))
		{
			if (trimmed === '')
			{
				return I18n.defaultValue('config_invalid_number_empty', 'Enter a number (the default is %1).').replace('%1', option.defvalue);
			}
			if (!/^-?[0-9]+$/.test(trimmed))
			{
				return I18n.defaultValue('config_invalid_number', 'Enter a whole number, digits only (for example %1).').replace('%1', option.defvalue);
			}
			var n = parseInt(trimmed, 10);
			if (rule.min !== null && (n < rule.min || n > rule.max))
			{
				return I18n.defaultValue('config_invalid_range', 'Enter a number from %1 to %2.').replace('%1', rule.min).replace('%2', rule.max);
			}
		}
		var yesNo = option.select && option.select.length === 2 &&
			option.select.map(function(v) { return v.toLowerCase(); }).sort().join() === 'no,yes';
		var aliases = CHOICE_ALIASES[family] || [];
		if (option.select && option.select.length > 1 && option.type === 'switch' && trimmed !== '' &&
			!option.select.some(function(v) { return v.toLowerCase() === trimmed.toLowerCase(); }) &&
			aliases.indexOf(trimmed.toLowerCase()) === -1 &&
			!(yesNo && /^(true|false|on|off|0|1|enable|disable|enabled|disabled)$/i.test(trimmed)))
		{
			return I18n.defaultValue('config_invalid_choice', 'The file says "%1", which is not a choice. Pick one of: %2.')
				.replace('%1', trimmed).replace('%2', option.select.join(', '));
		}
		if (REQUIRED_DIRS.indexOf(family) > -1 && trimmed === '')
		{
			return I18n.defaultValue('config_invalid_folder_empty', 'Enter a folder. It must not be empty.');
		}
		if (family === 'server1.host' && serverActive(option.name))
		{
			if (trimmed === '')
			{
				return I18n.defaultValue('config_invalid_host_empty', 'Enter the server address (for example news.example.com), or turn the server off.');
			}
			if (/^[a-z]+:\/\//i.test(trimmed))
			{
				return I18n.defaultValue('config_invalid_host_scheme', 'Enter only the name, without http:// or nntp://.');
			}
			if (!HOST_RE.test(trimmed))
			{
				return I18n.defaultValue('config_invalid_host', 'This is not a server address. Use letters, digits, dots and dashes.');
			}
		}
		if (CHECKED_DIRS.indexOf(family) > -1 && trimmed !== '')
		{
			var check = pathChecks[pathKey(option, value)];
			if (check && check !== 'pending' && check.state === 'bad')
			{
				return check.message;
			}
		}
		return '';
	}

	function mainDirValue()
	{
		var main = findOptionByName('MainDir');
		return main ? getOptionValue(main) : '';
	}

	function pathKey(option, value)
	{
		return value + '\u0001' + (familyName(option.name) === 'maindir' ? '' : mainDirValue());
	}

	function checkPathOption(option, value)
	{
		var key = pathKey(option, value);
		if (pathChecks[key]) return;
		pathChecks[key] = 'pending';
		RPC.call('checkpath', [value, familyName(option.name) === 'maindir' ? '' : mainDirValue()], function(result)
		{
			pathChecks[key] = result && result.State ? result : { state: 'ok' };
			pathChecks[key].state = result && result.State;
			pathChecks[key].message = result && result.Message;
			scheduleValidation();
		}, function()
		{
			// an older daemon without checkpath: no folder check
			pathChecks[key] = { state: 'ok' };
		});
	}

	function scheduleValidation()
	{
		clearTimeout(validationTimer);
		validationTimer = setTimeout(refreshValidation, 60);
	}

	function refreshValidation()
	{
		if (!config || !$ConfigData)
		{
			return;
		}
		invalidOptions = [];
		eachEditableOption(function(option)
		{
			if (option.type === 'info' || option.nocontent)
			{
				return;
			}
			var control = $('#' + option.formId);
			if (!control.length)
			{
				return;
			}
			var value = getOptionValue(option);
			var family = familyName(option.name);
			if (CHECKED_DIRS.indexOf(family) > -1 && $.trim(value || '') !== '')
			{
				checkPathOption(option, value);
			}
			var problem = optionProblem(option, value);
			var group = control.closest('.control-group');
			var box = group.find('> .controls > .config-invalid');
			if (problem)
			{
				invalidOptions.push(option);
				if (!box.length)
				{
					box = $('<div class="config-invalid" role="alert"></div>').attr('id', option.formId + '_Invalid');
					var controls = group.children('.controls');
					var help = controls.children('.config-help-toggle, .help-block').first();
					if (help.length) box.insertBefore(help); else controls.append(box);
				}
				if (box.text() !== problem) box.text(problem);
				group.addClass('has-invalid');
				control.attr('aria-invalid', 'true').attr('aria-describedby', option.formId + '_Invalid');
			}
			else if (box.length || group.hasClass('has-invalid'))
			{
				box.remove();
				group.removeClass('has-invalid');
				control.removeAttr('aria-invalid').removeAttr('aria-describedby');
			}
		});
		var count = invalidOptions.length;
		var why = count === 0 ? '' : (count === 1 ? I18n.defaultValue('config_fix_one', 'Fix 1 setting before saving') :
			I18n.defaultValue('config_fix_n', 'Fix %1 settings before saving').replace('%1', count));
		$('[id=Config_Save], #ConfigSaveBar_Save').prop('disabled', count > 0 || pageSaving || serverSaving).attr('title', why || null);
		if ($SaveBar)
		{
			var $fix = $('.config-savebar__fix', $SaveBar);
			if (!$fix.length)
			{
				$fix = $('<button type="button" class="btn-link config-savebar__fix"></button>').on('click', function()
				{
					focusFirstInvalid();
				});
				$fix.insertBefore($('#ConfigSaveBar_Discard', $SaveBar));
			}
			$fix.text(why).prop('hidden', count === 0);
			if (count > 0) $SaveBar.prop('hidden', false);
		}
	}
	this.refreshValidation = refreshValidation;
	this.invalidCount = function() { return invalidOptions.length; };

	function focusFirstInvalid()
	{
		var option = invalidOptions[0];
		if (!option) return;
		if (option.sectionId && Config.currentSectionID !== option.sectionId)
		{
			Config.showSection(option.sectionId, false);
		}
		var control = $('#' + option.formId);
		var group = control.closest('.control-group');
		if (group.hasClass('list-closed'))
		{
			group.removeClass('list-closed');
		}
		setTimeout(function() { scrollOptionIntoView(option.formId); control.focus(); }, 50);
	}

	/*** RESTORE PREVIOUS SETTINGS ***********************************************/

	this.restorePrevious = function()
	{
		RPC.call('configbackups', [], function(list)
		{
			list = list || [];
			if (!list.length)
			{
				AlertDialog.showModal(I18n.defaultValue('config_restore_none_title', 'No earlier settings yet'),
					Util.textToHtml(I18n.defaultValue('config_restore_none', 'RustyGet keeps a copy of your settings every time you save them. There is no copy yet.')));
				return;
			}
			var options = list.map(function(b, i)
			{
				var when = b.Time ? new Date(b.Time * 1000).toLocaleString() : b.Name;
				return '<option value="' + Util.textToAttr(b.Name) + '"' + (i === 0 ? ' selected' : '') + '>' +
					Util.textToHtml(when + (i === 0 ? ' (' + I18n.defaultValue('config_restore_newest', 'newest') + ')' : '')) + '</option>';
			}).join('');
			ConfirmDialog.showModal('RestorePreviousConfirmDialog', function(dialog)
			{
				var name = $('#RestorePrevious_Choice', dialog).val() || list[0].Name;
				RPC.call('restoreconfig', [name], function()
				{
					Config.reload();
				}, function(message, result)
				{
					AlertDialog.showModal(I18n.defaultValue('config_restore_failed', 'Could not restore the settings'),
						Util.textToHtml(result && result.error ? result.error.message : message));
				});
			}, function(dialog)
			{
				$('#ConfirmDialog_Text', dialog).append('<p><label for="RestorePrevious_Choice">' +
					Util.textToHtml(I18n.defaultValue('config_restore_choose', 'Settings saved on:')) + '</label> ' +
					'<select id="RestorePrevious_Choice" style="width:auto;max-width:100%">' + options + '</select></p>');
			});
		}, function(message, result)
		{
			AlertDialog.showModal(I18n.defaultValue('config_restore_failed', 'Could not restore the settings'),
				Util.textToHtml(result && result.error ? result.error.message : message));
		});
	};

	var $SaveBar = null;
	var changeTimer = null;

	function ensureSaveBar()
	{
		if ($SaveBar)
		{
			return;
		}
		$SaveBar = $('<div class="config-savebar" id="ConfigSaveBar" role="region" hidden>' +
			'<span class="config-savebar__count"></span>' +
			'<span class="config-savebar__names"></span>' +
			'<button type="button" class="btn" id="ConfigSaveBar_Discard"></button>' +
			'<button type="button" class="btn btn-primary" id="ConfigSaveBar_Save"></button>' +
			'</div>');
		$SaveBar.attr('aria-label', I18n.defaultValue('config_unsaved_changes', 'Unsaved changes'));
		$('#ConfigSaveBar_Discard', $SaveBar).text(I18n.defaultValue('btn_discard', 'Discard')).on('click', function()
		{
			ConfirmDialog.showModal('DiscardChangesConfirmDialog', discardOptionChanges);
		});
		$('#ConfigSaveBar_Save', $SaveBar).text(I18n.defaultValue('btn_save_and_reload', 'Save and reload')).on('click', function() { Config.saveChanges(); });
		$('.config__main', $ConfigContent).append($SaveBar);
		// Listen on the document, not only on #ConfigData: option editors in dialogs (RSS
		// filter, script lists, list deletes) change controls from outside the page.
		$(document).on('input change keyup click', function()
		{
			if (!config)
			{
				return;
			}
			clearTimeout(changeTimer);
			changeTimer = setTimeout(refreshChangeState, 120);
		});
		// number boxes: the stepper buttons, and only digits (a minus where allowed)
		$ConfigData.on('click', '.config-number .config-step', function()
		{
			var wrapper = $(this).closest('.config-number');
			var input = wrapper.find('input.editnumeric');
			var value = parseInt(input.val(), 10);
			setNumber(input, (isNaN(value) ? 0 : value) + parseInt($(this).attr('data-step'), 10), wrapper.data('meta'));
			syncSlider(wrapper);
		});
		$ConfigData.on('input', '.config-number input.editnumeric', function()
		{
			// a reference to another option, such as ${Server1.Connections}, is kept as typed
			if (/^\s*\$/.test(this.value))
			{
				return;
			}
			var allowMinus = $(this).attr('pattern') === '-?[0-9]*';
			var clean = this.value.replace(allowMinus ? /(?!^-)[^0-9]/g : /[^0-9]/g, '');
			if (clean !== this.value)
			{
				var pos = Math.max(0, (this.selectionStart || 0) - (this.value.length - clean.length));
				this.value = clean;
				try { this.setSelectionRange(pos, pos); } catch (e) { /* not focused */ }
			}
			syncSlider($(this).closest('.config-number'));
		});
		$ConfigData.on('keydown', '.config-number input.editnumeric', function(e)
		{
			if (e.key === 'ArrowUp' || e.key === 'ArrowDown')
			{
				e.preventDefault();
				$(this).closest('.config-number').find('.config-step[data-step="' + (e.key === 'ArrowUp' ? 1 : -1) + '"]').click();
			}
		});
		$ConfigData.on('change', '.config-number input.editnumeric', function()
		{
			var meta = $(this).closest('.config-number').data('meta');
			if (meta && this.value !== '' && !/^\s*\$/.test(this.value))
			{
				var n = parseInt(this.value, 10);
				var clamped = isNaN(n) ? meta.hardMin : clampNumber(n, meta);
				if (clamped !== n)
				{
					this.value = String(clamped);
					syncSlider($(this).closest('.config-number'));
				}
			}
		});
		$ConfigData.on('keydown', '.btn-switch.is-toggle', function(e)
		{
			if (e.key === ' ' || e.key === 'Enter')
			{
				e.preventDefault();
				Config.toggleClick(this);
			}
		});
	}

	function toggleIsOn(group)
	{
		return String(switchGetValue($(group)) || '').toLowerCase() === 'yes';
	}

	function refreshChangeState()
	{
		ensureSaveBar();
		var state = changedOptions();
		$ConfigData.find('.control-group.is-changed').removeClass('is-changed');
		state.options.forEach(function(option)
		{
			$('#' + option.formId).closest('.control-group').addClass('is-changed');
		});
		$ConfigData.find('.btn-switch.is-toggle').each(function()
		{
			$(this).attr('aria-checked', toggleIsOn(this) ? 'true' : 'false');
		});
		var count = state.options.length;
		var dirty = count > 0 || state.lists || state.restored;
		$SaveBar.prop('hidden', !dirty);
		$ConfigContent.toggleClass('has-unsaved', dirty);
		if (dirty)
		{
			var countText = count === 1 ? I18n.defaultValue('config_one_change', '1 change') :
				I18n.defaultValue('config_n_changes', '%1 changes').replace('%1', count);
			if (count === 0)
			{
				countText = state.restored ? I18n.defaultValue('config_restored_changes', 'Restored settings') :
					I18n.defaultValue('config_list_changes', 'List changes');
			}
			$('.config-savebar__count', $SaveBar).text(countText);
			$('.config-savebar__names', $SaveBar).text(state.options.slice(0, 4).map(function(option)
			{
				return option.label || option.caption;
			}).join(', ') + (count > 4 ? ' \u2026' : ''));
		}
		refreshControls();
		refreshValidation();
	}
	this.refreshChangeState = refreshChangeState;

	// Discard reloads Settings from RustyGet, which also drops added, deleted or
	// reordered list entries and a restored backup, then returns to the same section.
	function discardOptionChanges()
	{
		var section = Config.currentSectionID;
		restored = false;
		Config.hide();
		Config.show();
		Options.loadConfig({
			complete: function(_config)
			{
				Config.buildPage(_config);
				if (section && section !== 'Config-Info' && (/^Config-/.test(section) || findSectionById(section)))
				{
					Config.showSection(section, false);
				}
			},
			configError: Config.loadConfigError,
			serverTemplateError: Config.loadServerTemplateError
		});
	}

	this.switchClick = function(control)
	{
		var btn  = $('.btn', $(control).parent());
		btn.removeClass('btn-primary').attr('aria-pressed', 'false');
		btn.addClass('btn-default');
		$(control).addClass('btn-primary').attr('aria-pressed', 'true');
		$(control).removeClass('btn-default');

		// not for page Postprocess in download details
		if (config)
		{
			var optFormId = $(control).parent().attr('id');
			var option = findOptionById(optFormId);
			
			if (option.onchange)
			{
				option.onchange(option);
			}

			tlsSwitchHelper(option)
			if (optionFamily(option.name) === 'Server.IpVersion')
			{
				rerenderHostLookup(option);
			}
			refreshListRows();
		}
	}

	function tlsSwitchHelper(option)
	{
		var defaultPort = '119';
		var defaultTlsPort = '563';

		var defaultPort2 = '80';
		var defaultTlsPort2 = '443';
		
		var suffixStartIdx = option.formId.indexOf('_Encryption');
		if (suffixStartIdx < 0)
		{
			return;
		}

		var portOptionId = option.formId.substring(0, suffixStartIdx) + '_Port';
		var portOption = findOptionById(portOptionId);
		var useTls = getOptionValue(option) === 'yes';
		var currentPort = getOptionValue(portOption);
		var inputField = $('#' + portOptionId)[0];
		tlsSwitchPort(option, useTls, currentPort, inputField, defaultPort, defaultTlsPort, defaultPort2, defaultTlsPort2);
		if (inputField && inputField.value !== currentPort)
		{
			// the host lookup times the new port
			$(inputField).trigger('change.lookup');
		}
	}

	function tlsSwitchPort(option, useTls, currentPort, inputField, defaultPort, defaultTlsPort, defaultPort2, defaultTlsPort2)
	{
		if (useTls)
		{
			if (currentPort === defaultPort)
			{
				inputField.value = defaultTlsPort;
				return;
			}

			if (currentPort === defaultPort2)
			{
				inputField.value = defaultTlsPort2;
			}

			return;
		}

		if (!useTls)
		{
			if (currentPort === defaultTlsPort)
			{
				inputField.value = defaultPort;
				return;
			}

			if (currentPort === defaultTlsPort2)
			{
				inputField.value = defaultPort2;
			}

			return;
		}
	}

	function switchGetValue(control)
	{
		var btn = $('.btn-primary', control);
		var state = btn.attr('data-value') !== undefined ? btn.attr('data-value') : btn.val();
		return state;
	}

	function switchSetValue(control, value)
	{
		$('.btn', control).removeClass('btn-primary').attr('aria-pressed', 'false');
		var btn = $('.btn', control).filter(function() { return $(this).attr('data-value') == value || $(this).val() == value || $(this).attr('value') == value; });
		btn.addClass('btn-primary').attr('aria-pressed', 'true');
	}

	this.togglePassword = function(control, target)
	{
		var checked = $(control).is(':checked');
		$('#'+target).prop('type', checked ? 'text' : 'password');
	}

	/*** CHANGE/ADD/REMOVE OPTIONS *************************************************************/

	function navClick(event)
	{
		event.preventDefault();
		var sectionId = $(this).attr('href').substr(1);
		Config.showSection(sectionId, true);
	}

	function showOptionRows(rows)
	{
		rows.show();
		// Closed list rows are hidden with !important, so jQuery cannot infer
		// their grid display when a section or search reveals them.
		rows.filter('.config-option-tile').css('display', 'grid');
	}

	this.showSection = function(sectionId, animateScroll)
	{
		closeServerEditor();
		this.currentSectionID = sectionId;
		var link = $('a[href="#' + sectionId + '"]', $ConfigNav);
		$('li', $ConfigNav).removeClass('active');
		link.closest('li').addClass('active');
		// Search and health links select sections without focusing the menu.
		// Scroll only the bounded menu so the option's page position stays intact.
		if ($('body').hasClass('phone') && link.length)
		{
			var menu = $ConfigNav[0];
			var menuRect = menu.getBoundingClientRect();
			var linkRect = link[0].getBoundingClientRect();
			var menuTop = menuRect.top + menu.clientTop;
			var menuBottom = menuTop + menu.clientHeight;
			if (linkRect.top < menuTop)
			{
				menu.scrollTop += linkRect.top - menuTop;
			}
			else if (linkRect.bottom > menuBottom)
			{
				menu.scrollTop += linkRect.bottom - menuBottom;
			}
		}
		$ConfigContent.removeClass('search');
		Util.show($ViewButton, sectionId !== 'Config-Info');

		$ConfigInfo.hide();
		$ConfigLicenses.hide();

		if (sectionId === 'Search')
		{
			search();
			return;
		}

		lastSection = sectionId;

		if (sectionId === 'Config-Info')
		{
			renderOverview();
			$ConfigInfo.show();
			$ConfigData.children().hide();
			$ConfigTitle.attr('data-i18n', 'nav_overview');
			I18n.translatePage($ConfigTitle.parent());
			$ConfigTitleStatus.hide();
			return;
		}

		if (sectionId === 'Config-SystemInfo')
		{
			$ConfigData.children().hide();
			$('.config-status', $ConfigData).show();
			SystemInfo.loadSystemInfo();
			$ConfigTitle.attr('data-i18n', 'nav_status');
			I18n.translatePage($ConfigTitle.parent());
			$ConfigTitleStatus.show();
			return;
		}

		if (sectionId === 'Config-Licenses')
		{
			$ConfigLicenses.show();
			$ConfigData.children().hide();
			$ConfigTitle.attr('data-i18n', 'config_title_licenses');
			I18n.translatePage($ConfigTitle.parent());
			return;
		}

		if (sectionId === 'Config-System')
		{
			$ConfigData.children().hide();
			$('.config-system', $ConfigData).show();
			markLastControlGroup();
			$ConfigTitle.attr('data-i18n', 'nav_system');
			I18n.translatePage($ConfigTitle.parent());
			$ConfigTitleStatus.hide();
			return;
		}

		$ConfigData.children().hide();
		var opts = $('.' + sectionId, $ConfigData);
		showOptionRows(opts);
		markLastControlGroup();

		var section = findSectionById(sectionId);
		var sectionText = section.caption ? section.caption : section.name;
		var categoryName = I18n.defaultValue(('config_section_' + section.name.replace(/ /g, '_').replace(/-/g, '_')).toLowerCase(), sectionText);
		$ConfigTitle.removeAttr('data-i18n').removeData('i18n-last');
		$ConfigTitle.text(sentenceCase(categoryName));
		$ConfigTitle.attr('data-path', 'rustyget::settings::' + sectionId.toLowerCase().replace(/[^a-z0-9]+/g, '_'));
		I18n.translatePage($ConfigTitle.parent());
		$ConfigTitleStatus.hide();

		$Body.animate({ scrollTop: 0 }, { duration: animateScroll ? 'slow' : 0, easing: 'swing' });
	}

	this.deleteSet = function(control, setname, sectionId)
	{
		var multiid = parseInt($(control).attr('data-multiid'));
		if (sectionId === 'NEWS-SERVERS') closeServerEditor();
		$('#ConfigDeleteConfirmDialog_Option').text(setname + multiid);
		ConfirmDialog.showModal('ConfigDeleteConfirmDialog', function()
		{
			deleteOptionSet(setname, multiid, sectionId);
		});
	}

	function deleteOptionSet(setname, multiid, sectionId)
	{
		// remove options from page, using a temporary div for slide effect
		var opts = $('.' + sectionId + '.multiid' + multiid, $ConfigData);
		var div = $('<div></div>');
		opts.first().before(div);
		div.append(opts);
		div.slideUp('normal', function()
		{
			div.remove();
		});

		// remove option set from config
		var section = findSectionById(sectionId);
		for (var j=0; j < section.options.length; j++)
		{
			var option = section.options[j];
			if (!option.template && option.multiid === multiid)
			{
				section.options.splice(j, 1);
				j--;
			}
		}

		// reformat remaining sets (captions, input IDs, etc.)
		reformatSection(section, setname);

		section.modified = true;
		if (typeof refreshChangeState === 'function') refreshChangeState();
	}

	function reformatSection(section, setname)
	{
		var hasOptions = false;
		var lastMultiId = 0;
		for (var j=0; j < section.options.length; j++)
		{
			var option = section.options[j];
			if (!option.template)
			{
				if (option.multiid !== lastMultiId && option.multiid !== lastMultiId + 1)
				{
					reformatSet(section, setname, option.multiid, lastMultiId + 1);
				}
				lastMultiId = option.multiid;
				hasOptions = true;
			}
		}

		// update add-button
		var addButton = $('.config-add.' + section.id, $ConfigData);
		addButton.html('<span data-i18n="label_add"></span> ' +
			(hasOptions ? '<span data-i18n="label_another"></span> ' : '') +
			setname);
		I18n.translatePage(addButton);
		refreshListRows();
	}

	function reformatSet(section, setname, oldMultiId, newMultiId)
	{
		for (var j=0; j < section.options.length; j++)
		{
			var option = section.options[j];
			if (!option.template && option.multiid == oldMultiId)
			{
				// reformat multiid
				var div = $('#' + setname + oldMultiId);
				div.attr('id', setname + newMultiId);

				// update captions
				$('.config-settitle.' + section.id + '.multiid' + oldMultiId, $ConfigData).not('.config-list-row').text(setname + newMultiId);
				$('.config-list-row.' + section.id + '.multiid' + oldMultiId, $ConfigData).attr('data-multiid', newMultiId);
				$('.' + section.id + '.multiid' + oldMultiId + ' .config-multicaption', $ConfigData).text(setname + newMultiId + '.');

				var delBtn = $('.' + section.id + '.multiid' + oldMultiId + ' .config-delete', $ConfigData);
				delBtn.html('<span data-i18n="btn_delete"></span> ' + setname + newMultiId);
				I18n.translatePage(delBtn);

				//update data id
				$('.' + section.id + '.multiid' + oldMultiId + ' .config-button', $ConfigData).attr('data-multiid', newMultiId);

				//update class
				$('.' + section.id + '.multiid' + oldMultiId, $ConfigData).removeClass('multiid' + oldMultiId).addClass('multiid' + newMultiId);

				// update input id
				var oldFormId = option.formId;
				option.formId = option.formId.replace(new RegExp(option.multiid), newMultiId);
				var optionControl = $('#' + oldFormId);
				optionControl.attr('id', option.formId);
				if (optionControl.hasClass('btn-switch'))
				{
					optionControl.attr('aria-labelledby', 'label:' + option.formId);
				}

				// update label data-optid
				$('a[data-optid=' + oldFormId + ']').attr('data-optid', option.formId).attr('id', 'label:' + option.formId);
				var row = optionControl.closest('.control-group');
				row.find('.config-host-lookup').attr('data-multiid', newMultiId);
				row.find('.help-block').attr('id', 'help:' + option.formId);
				row.find('.config-help-toggle').attr('aria-controls', 'help:' + option.formId)
					.attr('aria-label', I18n.translate('btn_quick_help') + ': ' + setname + newMultiId + '.' + option.caption.split('.').slice(1).join('.'));

				// update editor id
				$('#' + oldFormId + '_Editor').attr('id', option.formId + '_Editor');

				// update name
				option.name = option.name.replace(new RegExp(option.multiid), newMultiId);

				option.multiid = newMultiId;
			}
		}
	}

	this.addSet = function(setname, sectionId)
	{
		// find section
		var section = findSectionById(sectionId);

		// find max multiid
		var multiid = 0;
		for (var j=0; j < section.options.length; j++)
		{
			var option = section.options[j];
			if (!option.template && option.multiid > multiid)
			{
				multiid = option.multiid;
			}
		}
		multiid++;

		// create new multi set
		var addedOptions = [];
		for (var j=0; j < section.options.length; j++)
		{
			var option = section.options[j];
			if (option.template)
			{
				var name = option.name.replace(/1/, multiid);
				// copy option
				var newoption = $.extend({}, option);
				newoption.name = name;
				newoption.caption = option.caption.replace(/1/, multiid);
				newoption.template = false;
				newoption.multiid = multiid;
				section.options.push(newoption);
				addedOptions.push(newoption);
			}
		}

		section.modified = true;
		if (typeof refreshChangeState === 'function') refreshChangeState();

		// visualize new multi set
		var html = buildMultiSetContent(section, multiid);

		var addButton = $('.config-add.' + section.id, $ConfigData);
		addButton.html('<span data-i18n="label_add"></span> ' + 
			'<span data-i18n="label_another"></span> ' + setname);
		I18n.translatePage(addButton);

		// insert before add-button, using a temporary div for slide effect
		var div = $('<div class="config-set-insertion">' + html + '</div>');
		I18n.translatePage(div);
		div.hide();
		addButton.parent().before(div);
		enhanceControls(div);
		applyMode();
		if (listSectionPrefix(sectionId))
		{
			setListEditor(sectionId, multiid, true);
		}

		for (var j=0; j < addedOptions.length; j++)
		{
			var option = addedOptions[j];
			if (option.onchange)
			{
				option.onchange(option);
			}
		}

		div.slideDown('normal', function()
		{
			var opts = div.children();
			opts.detach();
			div.after(opts);
			div.remove();
		});
	}

	this.moveSet = function(control, setname, sectionId, direction)
	{
		var id1 = parseInt($(control).attr('data-multiid'));
		var id2 = direction === 'down' ? id1 + 1 : id1 - 1;
		if (sectionId === 'NEWS-SERVERS') closeServerEditor();

		// swap options in two sets
		var opts1 = $('.' + sectionId + '.multiid' + (direction === 'down' ? id1 : id2), $ConfigData);
		var opts2 = $('.' + sectionId + '.multiid' + (direction === 'down' ? id2 : id1), $ConfigData);

		if (opts1.length === 0 || opts2.length === 0)
		{
			return;
		}

		opts1.first().before(opts2);

		// reformat remaining sets (captions, input IDs, etc.)
		var section = findSectionById(sectionId);
		reformatSet(section, setname, id2, 10000 + id2);
		reformatSet(section, setname, id1, id2);
		reformatSet(section, setname, 10000 + id2, id1);

		section.modified = true;
		if (typeof refreshChangeState === 'function') refreshChangeState();
		refreshListRows();
	}

	this.viewMode = function()
	{
		compactMode = !compactMode;
		UISettings.write('$Config_ViewCompact', compactMode ? 'yes' : 'no');
		setViewMode();
	}

	function setViewMode()
	{
		if (!compactMode)
		{
			$('#Config_ViewCompact > .material-icon').text('check_box_outline_blank');
		}
		else
		{
			$('#Config_ViewCompact > .material-icon').text('check_box');
		}
		
		$ConfigContent.toggleClass('hide-help-block', compactMode);
		$ConfigData.find('.control-group:has(.config-help-toggle)').toggleClass('help-expanded', !compactMode);
		$ConfigData.find('.config-help-toggle').attr('aria-expanded', !compactMode);
	}

	this.toggleHelp = function(control)
	{
		var row = $(control).closest('.control-group');
		var expanded = !row.hasClass('help-expanded');
		row.toggleClass('help-expanded', expanded);
		$(control).attr('aria-expanded', expanded);
	}

	function refreshListRows()
	{
		$('.config-list-row', $ConfigData).each(function()
		{
			var row = $(this);
			var multiid = row.attr('data-multiid');
			var sectionId = row.attr('data-section');
			var prefix = listSectionPrefix(sectionId);
			function value(name)
			{
				var option = findOptionByName(prefix + multiid + '.' + name);
				return option ? getOptionValue(option) || '' : '';
			}
			var name = value('Name') || prefix + multiid;
			var secondary = '', status = '', extra = '';
			if (prefix === 'Server')
			{
				secondary = value('Host') + (value('Port') ? ':' + value('Port') : '');
				status = I18n.translate(value('Active') === 'yes' ? 'label_active' : 'label_disabled');
				extra = value('Connections') + ' ' + I18n.translate('stat_server_connections');
			}
			else if (prefix === 'Category')
			{
				secondary = value('Aliases');
				status = I18n.translate('config_section_unpack') + ': ' + I18n.translate(value('Unpack') === 'yes' ? 'label_yes_cap' : 'label_no_cap');
			}
			else if (prefix === 'Feed')
			{
				secondary = value('Category');
				status = value('Interval') + ' ' + I18n.translate('config_unit_minutes');
			}
			else if (prefix === 'Task')
			{
				secondary = value('Time');
				status = value('WeekDays');
				var command = findOptionByName(prefix + multiid + '.Command');
				extra = command ? $('#' + command.formId + ' .btn-primary').val() || value('Command') : '';
			}
			row.find('.config-list-name').text(name);
			row.find('.config-list-host').text(secondary).toggle(!!secondary);
			row.find('.config-list-status').text(status).toggle(!!status);
			row.find('.config-list-connections').text(extra).toggle(!!extra);
			var details = $('.config-list-detail.' + sectionId + '.multiid' + multiid, $ConfigData);
			var issues = details.find('.option-alert').length;
			row.find('.config-list-health').text(issues ? issues + ' ' + I18n.translate('config_server_issues') : '').toggle(issues > 0);
			var edit = row.find('.config-list-edit');
			var caption = I18n.translate(edit.attr('aria-expanded') === 'true' ? 'btn_close' : 'btn_edit');
			if (row.hasClass('is-editing') && $ConfigContent.hasClass('server-page-open'))
			{
				caption = I18n.defaultValue('config_server_back', 'Back to servers');
			}
			edit.text(caption).attr('aria-label', caption + ': ' + name);
			edit.attr('aria-controls', details.find('input[id],.btn-switch[id]').map(function() { return this.id; }).get().join(' '));
		});
		refreshServerPane();
	}

	this.refreshHealth = function()
	{
		if (!config || !$ConfigData) return;
		// Health may arrive after Settings has rendered. Update decorations only;
		// rebuilding the page would replace edits, focus and disclosure state.
		for (var k=0; k < config.length; k++)
		{
			var sections = config[k].sections;
			for (var i=0; i < sections.length; i++)
			{
				var section = sections[i];
				var health = SystemHealth.isHealthCheckEnabled() ? SystemHealth.getSection(section.id) : null;
				var link = $ConfigNav.find('a').filter(function() { return $(this).attr('href') === '#' + section.id; });
				link.children('.config-health-badges').remove();
				var badges = SystemHealth.makeBadges(health);
				if (badges) link.append(badges.addClass('config-health-badges'));
				for (var j=0; j < section.options.length; j++)
				{
					var option = section.options[j];
					if (option.template || !option.formId) continue;
					option.check = SystemHealth.getCheck(health, option.name);
					var row = $(document.getElementById(option.formId)).closest('.control-group');
					row.find('.option__check-section').remove();
					if (option.check) row.children('.controls').append(makeOptionCheckSection(option.check));
				}
			}
		}
		refreshListRows();
		refreshLayoutHealth();
	}

	function listSectionPrefix(sectionId)
	{
		return {'NEWS-SERVERS': 'Server', CATEGORIES: 'Category', RSS_FEEDS: 'Feed', SCHEDULER: 'Task'}[sectionId];
	}

	function setListEditor(sectionId, multiid, expanded)
	{
		if (sectionId === 'NEWS-SERVERS' && !$ConfigContent.hasClass('search'))
		{
			if (expanded) openServerEditor(multiid);
			else if (serverEditing === parseInt(multiid)) closeServerEditor(true);
			else setListEditorRaw(sectionId, multiid, false);
			return;
		}
		setListEditorRaw(sectionId, multiid, expanded);
	}

	function setListEditorRaw(sectionId, multiid, expanded)
	{
		$('.config-list-row.' + sectionId + '.multiid' + multiid + ' .config-list-edit', $ConfigData).attr('aria-expanded', expanded);
		$('.config-list-detail.' + sectionId + '.multiid' + multiid, $ConfigData).toggleClass('list-closed', !expanded);
		refreshListRows();
	}

	this.editList = function(control)
	{
		var row = $(control).closest('.config-list-row');
		setListEditor(row.attr('data-section'), row.attr('data-multiid'), $(control).attr('aria-expanded') !== 'true');
	}

	/*** SERVER EDITOR: SIDE PANE OR OWN PAGE ****************************************/

	// A wide window edits a server in a pane on the right, beside the list. A narrow
	// one gives the server the whole page. Either way "Add server" is only offered
	// from the list, and Save writes the server to rustyget.conf straight away.
	var SERVER_PANE_MIN_WIDTH = 1180;
	var serverEditing = 0;
	var serverSaving = false;
	// while a server is being saved, the page-wide save waits: its write could be
	// overtaken by the server's and lose the other changes
	var pageSaving = false;
	function setServerSaving(on)
	{
		serverSaving = on;
		$('#ConfigSaveBar_Save, [id=Config_Save]').prop('disabled', on || pageSaving || invalidOptions.length > 0);
	}

	function serverDetails(multiid)
	{
		return $('.config-list-detail.NEWS-SERVERS.multiid' + multiid, $ConfigData);
	}

	function serverPaneWide()
	{
		return window.innerWidth >= SERVER_PANE_MIN_WIDTH;
	}

	function ensureServerPane()
	{
		var pane = $('#ConfigServerPane');
		if (pane.length)
		{
			return pane;
		}
		pane = $('<aside id="ConfigServerPane" class="config-pane" role="region">' +
			'<header class="config-pane__head"><div class="config-pane__titles"><h2 class="config-pane__title"></h2>' +
			'<p class="config-pane__host"></p></div>' +
			'<button type="button" class="btn config-pane__close"></button></header>' +
			'<div class="config-pane__body"></div></aside>');
		pane.find('.config-pane__close').on('click', function() { closeServerEditor(true); })
			.attr('aria-label', I18n.defaultValue('btn_close', 'Close')).html('<i class="material-icon">close</i>');
		$ConfigData.append(pane);
		return pane;
	}

	function refreshServerPane()
	{
		if (!serverEditing)
		{
			return;
		}
		var name = getOptionValue(findOptionByName('Server' + serverEditing + '.Name') || {}) || ('Server' + serverEditing);
		var host = findOptionByName('Server' + serverEditing + '.Host');
		var port = findOptionByName('Server' + serverEditing + '.Port');
		var where = (host ? getOptionValue(host) || '' : '') + (port && getOptionValue(port) ? ':' + getOptionValue(port) : '');
		$('#ConfigServerPane .config-pane__title').text(name);
		$('#ConfigServerPane .config-pane__host').text(where).toggle(!!where);
		$('#ConfigServerPane').attr('aria-label', name);
	}

	function openServerEditor(multiid)
	{
		multiid = parseInt(multiid);
		if (serverEditing && serverEditing !== multiid)
		{
			closeServerEditor();
		}
		if ($ConfigContent.hasClass('search'))
		{
			setListEditorRaw('NEWS-SERVERS', multiid, true);
			return;
		}
		var row = $('.config-list-row.NEWS-SERVERS.multiid' + multiid, $ConfigData);
		var details = serverDetails(multiid);
		if (!row.length || !details.length)
		{
			return;
		}
		serverEditing = multiid;
		$ConfigContent.addClass('server-editing');
		row.addClass('is-editing');
		var wide = serverPaneWide();
		$ConfigContent.toggleClass('server-pane-open', wide).toggleClass('server-page-open', !wide);
		if (wide)
		{
			var pane = ensureServerPane();
			if (!row.next('.config-pane-anchor').length)
			{
				row.after('<span class="config-pane-anchor" hidden></span>');
			}
			pane.find('.config-pane__body').append(details);
		}
		setListEditorRaw('NEWS-SERVERS', multiid, true);
		refreshServerPane();
		setServerStatus('');
		if (wide)
		{
			$('#ConfigServerPane .config-pane__body').scrollTop(0);
		}
		else
		{
			$('.config__main').scrollTop(0);
			$('html').scrollTop(0);
		}
	}

	function closeServerEditor(restoreFocus)
	{
		var multiid = serverEditing;
		if (!multiid)
		{
			return;
		}
		var row = $('.config-list-row.NEWS-SERVERS.multiid' + multiid, $ConfigData);
		var anchor = row.next('.config-pane-anchor');
		var details = serverDetails(multiid);
		if (anchor.length)
		{
			anchor.after(details);
			anchor.remove();
		}
		$('.config-pane-anchor', $ConfigData).remove();
		$('#ConfigServerPane').remove();
		row.removeClass('is-editing');
		$ConfigContent.removeClass('server-editing server-pane-open server-page-open');
		serverEditing = 0;
		setListEditorRaw('NEWS-SERVERS', multiid, false);
		if (restoreFocus) row.find('.config-list-edit').focus();
	}
	this.closeServerEditor = function() { closeServerEditor(true); };

	// a window that crosses the width limit moves the open editor between pane and page
	var serverResizeTimer = null;
	$(window).on('resize.serverpane', function()
	{
		clearTimeout(serverResizeTimer);
		serverResizeTimer = setTimeout(function()
		{
			if (serverEditing && serverPaneWide() !== $ConfigContent.hasClass('server-pane-open'))
			{
				var multiid = serverEditing;
				closeServerEditor();
				openServerEditor(multiid);
			}
		}, 150);
	});

	function setServerStatus(text, kind)
	{
		var status = $('.multiset-toolbar.NEWS-SERVERS.multiid' + serverEditing + ' .config-server-status', $ConfigData);
		status.text(text || '').toggle(!!text).attr('data-kind', kind || '');
	}

	function serverText(key, text)
	{
		return I18n.defaultValue('config_server_' + key, text);
	}

	// Everything else is written exactly as it was last saved (the saved values, not
	// the editor, so unsaved edits, additions or deletions elsewhere stay unsaved):
	// only the server list comes from the editor.
	function serverOnlySaveRequest()
	{
		var request = (config.values || []).filter(function(item)
		{
			return !/^Server\d+\./i.test(item.Name);
		}).map(function(item) { return { Name: item.Name, Value: item.Value }; });
		var section = findSectionById('NEWS-SERVERS');
		section.options.forEach(function(option)
		{
			if (option.template || option.type === 'info')
			{
				return;
			}
			var value = getOptionValue(option);
			if (value != null)
			{
				request.push({ Name: option.name, Value: value });
			}
		});
		return request;
	}

	function serverStructure()
	{
		return findSectionById('NEWS-SERVERS').options.filter(function(option) { return !option.template; })
			.map(function(option) { return option.name; }).join('\n');
	}

	function markServersSaved(request, structure)
	{
		var section = findSectionById('NEWS-SERVERS');
		var sent = {};
		request.forEach(function(item) { sent[item.Name.toLowerCase()] = item.Value; });
		// the saved baseline is what was written; an edit made while saving stays unsaved
		section.options.forEach(function(option)
		{
			if (!option.template && option.type !== 'info' && sent.hasOwnProperty(option.name.toLowerCase()))
			{
				option.value = sent[option.name.toLowerCase()];
			}
		});
		if (serverStructure() === structure)
		{
			section.modified = false;
		}
		// keep the saved copy in step so later saves and cipher checks compare against the file
		var saved = {};
		request.forEach(function(item) { saved[item.Name.toLowerCase()] = item.Value; });
		config.values = (config.values || []).filter(function(item)
		{
			return !/^Server\d+\./i.test(item.Name) || saved.hasOwnProperty(item.Name.toLowerCase());
		}).map(function(item)
		{
			var key = item.Name.toLowerCase();
			return saved.hasOwnProperty(key) ? { Name: item.Name, Value: saved[key] } : item;
		});
		request.forEach(function(item)
		{
			if (!config.values.some(function(value) { return value.Name.toLowerCase() === item.Name.toLowerCase(); }))
			{
				config.values.push({ Name: item.Name, Value: item.Value });
			}
		});
		refreshChangeState();
		refreshListRows();
	}

	this.saveServer = function(multiid, forceOptimize)
	{
		multiid = parseInt(multiid);
		if (serverSaving || pageSaving)
		{
			return;
		}
		if (typeof TlsCipherDialog !== 'undefined' && !TlsCipherDialog.canSaveSettings())
		{
			TlsCipherDialog.showSaveBlocked();
			return;
		}
		if (restored)
		{
			setServerStatus(serverText('restored_first', 'Settings were restored from a backup: use Save and reload first.'), 'error');
			return;
		}
		setServerSaving(true);
		$('.config-server-save, .config-cipher__optimize', $ConfigData).prop('disabled', true);
		setServerStatus(serverText('saving', 'Saving…'), 'busy');
		var request = serverOnlySaveRequest();
		var structure = serverStructure();
		var editor = config;
		RPC.call('saveconfig', [Config.withCipherBaseline(request)], function(ok)
		{
			if (config !== editor)
			{
				// Settings was closed or read again while saving: the file is written,
				// so put it to use, but leave the editor that is gone alone.
				if (ok)
				{
					applySavedServers();
				}
				else
				{
					setServerSaving(false);
				}
				return;
			}
			if (!ok)
			{
				serverSaveFailed(serverText('save_failed', 'Could not write the configuration file.'));
				return;
			}
			markServersSaved(request, structure);
			var encryption = findOptionByName('Server' + multiid + '.Encryption');
			var cipher = findOptionByName('Server' + multiid + '.Cipher');
			var wantsCipher = encryption && cipher && String(getOptionValue(encryption)).toLowerCase() === 'yes' &&
				(forceOptimize || !getOptionValue(cipher));
			if (wantsCipher)
			{
				optimizeSavedServer(multiid, !!forceOptimize);
			}
			else
			{
				applySavedServers();
			}
		}, function(value, result)
		{
			serverSaveFailed(result && result.error ? result.error.message : String(value));
		});
	};

	function serverSaveFailed(message)
	{
		setServerSaving(false);
		$('.config-server-save, .config-cipher__optimize', $ConfigData).prop('disabled', false);
		setServerStatus(message, 'error');
	}

	// Find the fastest cipher this server accepts and save it, before RustyGet reloads.
	var cipherWaitServer, cipherWaitForced;
	function optimizeSavedServer(multiid, forced)
	{
		cipherWaitServer = multiid;
		cipherWaitForced = forced;
		setServerStatus(serverText('cipher_testing', 'Finding the fastest cipher for this server…'), 'busy');
		var snapshot = Config.cipherSnapshot();
		var finish = function(note)
		{
			applySavedServers(note);
		};
		var misses = 0;
		var poll = function(job)
		{
			RPC.call('tlscipherstatus', [job], function(result)
			{
				misses = 0;
				if (['measuring', 'probing', 'saving'].indexOf(result.State) !== -1)
				{
					if (result.State === 'probing' && result.TotalProbes)
					{
						setServerStatus(serverText('cipher_testing', 'Finding the fastest cipher for this server…') +
							' ' + (result.CompletedProbes || 0) + ' / ' + result.TotalProbes, 'busy');
					}
					setTimeout(function() { poll(job); }, 500);
					return;
				}
				var server = (result.Servers || [])[0] || {};
				if (result.Saved === true && !Config.applyCipherResults(result, snapshot))
				{
					finish(serverText('cipher_conflict', 'A cipher was saved, but the editor changed meanwhile. Refresh Settings to see it.'));
				}
				else if (result.State === 'completed' && server.SelectedCipher)
				{
					finish(serverText('cipher_chosen', 'Cipher') + ': ' + server.SelectedCipher);
				}
				else
				{
					finish(forced ? (server.Message || result.Message || '') : '');
				}
			}, function()
			{
				// a missed status reply is retried; a reload would cancel a test still running
				if (++misses < 10)
				{
					setTimeout(function() { poll(job); }, 1000);
					return;
				}
				RPC.call('tlsciphercancel', [job], function() {}, function() {}, { timeout: 15000 });
				finish(serverText('cipher_lost', 'The cipher test stopped answering and was cancelled; the cipher is unchanged.'));
			}, { timeout: 15000 });
		};
		RPC.call('tlscipherstart', [String(multiid)], function(result)
		{
			waitedForCipher = 0;
			if (result.Available === false || !result.JobID)
			{
				finish(forced ? (result.Message || serverText('cipher_unavailable', 'Cipher testing is not available here.')) : '');
				return;
			}
			poll(result.JobID);
		}, function(value, result)
		{
			var message = result && result.error ? result.error.message : String(value);
			if (/already running/.test(message))
			{
				// another browser is testing other servers; a reload now would cancel its test
				waitForOtherCipherTest();
				return;
			}
			finish(forced ? message : '');
		}, { timeout: 15000 });
	}

	// Try again until the other cipher test is done, then test and reload as usual.
	// If it never ends, the server stays saved and is put to use with a later save or reload.
	var waitedForCipher = 0;
	function waitForOtherCipherTest()
	{
		if (waitedForCipher === 0)
		{
			waitedForCipher = new Date().getTime();
		}
		if (new Date().getTime() - waitedForCipher > 15 * 60 * 1000)
		{
			waitedForCipher = 0;
			setServerSaving(false);
			$('.config-server-save, .config-cipher__optimize', $ConfigData).prop('disabled', false);
			setServerStatus(serverText('cipher_busy_gave_up', 'Saved, but another cipher test is still running, so RustyGet was not reloaded. Save again once it finishes.'), 'error');
			return;
		}
		setServerStatus(serverText('cipher_busy', 'Saved. Waiting for another cipher test to finish before putting it to use…'), 'busy');
		setTimeout(function() { optimizeSavedServer(cipherWaitServer, cipherWaitForced); }, 2000);
	}

	// RustyGet reads servers when it starts, so a saved server is put to use with a reload.
	function applySavedServers(note)
	{
		var prefix = note ? note + ' ' : '';
		setServerStatus(prefix + serverText('applying', 'Saved. Putting it to use…'), 'busy');
		var before = Status.status && Status.status.UpTimeSec;
		var started = new Date();
		Refresher.pause();
		var done = function(text, kind)
		{
			setServerSaving(false);
			$('.config-server-save, .config-cipher__optimize', $ConfigData).prop('disabled', false);
			Refresher.resume(true);
			setServerStatus(prefix + text, kind);
		};
		var check = function()
		{
			RPC.call('status', [], function(status)
			{
				if (before === undefined || status.UpTimeSec < before)
				{
					Status.status = status;
					Options.refreshRuntime(function() { done(serverText('saved', 'Saved and in use.'), 'ok'); });
				}
				else if (new Date() - started > 60000)
				{
					done(serverText('saved_reload', 'Saved. Reload RustyGet to use it.'), 'ok');
				}
				else
				{
					setTimeout(check, 500);
				}
			}, function()
			{
				if (new Date() - started > 60000)
				{
					done(serverText('reload_unknown', 'Saved, but RustyGet did not answer after the reload. Check that it is running.'), 'error');
				}
				else
				{
					setTimeout(check, 500);
				}
			});
		};
		RPC.call('reload', [], function() { setTimeout(check, 500); }, function() { setTimeout(check, 500); });
	}

	// The cipher is a menu of the suites RustyGet can use, with "Optimize" beside it.
	var CIPHER_GROUPS = [
		['TLS 1.3', ['TLS_AES_128_GCM_SHA256', 'TLS_AES_256_GCM_SHA384', 'TLS_CHACHA20_POLY1305_SHA256']],
		['TLS 1.2', ['ECDHE-ECDSA-AES128-GCM-SHA256', 'ECDHE-RSA-AES128-GCM-SHA256', 'ECDHE-ECDSA-AES256-GCM-SHA384',
			'ECDHE-RSA-AES256-GCM-SHA384', 'ECDHE-ECDSA-CHACHA20-POLY1305', 'ECDHE-RSA-CHACHA20-POLY1305']]
	];

	function enhanceCipher(option, input)
	{
		var wrap = $('<div class="config-cipher"></div>');
		var select = $('<select class="config-select config-cipher__menu"></select>')
			.attr('aria-label', I18n.defaultValue('config_label_cipher', 'Cipher'));
		select.append($('<option value=""></option>').text(I18n.defaultValue('config_cipher_auto', 'Automatic (fastest is found when you save)')));
		CIPHER_GROUPS.forEach(function(group)
		{
			var optgroup = $('<optgroup></optgroup>').attr('label', group[0]);
			group[1].forEach(function(name) { optgroup.append($('<option></option>').attr('value', name).text(name)); });
			select.append(optgroup);
		});
		var button = $('<button type="button" class="btn config-cipher__optimize"></button>')
			.text(I18n.defaultValue('config_cipher_optimize', 'Optimize'))
			.attr('title', I18n.defaultValue('config_cipher_optimize_title', 'Save this server and pick the fastest cipher it accepts'))
			.on('click', function() { Config.saveServer(option.multiid, true); });
		input.before(wrap);
		wrap.append(select).append(button).append(input);
		input.addClass('config-cipher-source');
		select.on('change', function()
		{
			input.val(this.value).trigger('input').trigger('change');
			refreshChangeState();
		});
		syncCipher(select);
	}

	function syncCipher(select)
	{
		var value = $.trim(select.siblings('input').val() || '');
		if (!select.find('option').filter(function() { return this.value === value; }).length)
		{
			select.find('option.config-cipher__other').remove();
			select.append($('<option class="config-cipher__other"></option>').attr('value', value).text(value));
		}
		select.val(value);
	}

	/*** SERVER HOST LOOKUP ***********************************************************/

	// Typing a host name looks it up at once: every address it has, the country and
	// network of each (from the DB-IP Lite files on this machine, no IP leaves it)
	// and how long a connect takes. One address can be picked to always use.
	// The state lives on the lookup box, which moves with its server when servers
	// are reordered or deleted; the box's data-multiid is renumbered by reformatSet.
	var hostLookupSeq = 0;

	function hostText(key, text)
	{
		return I18n.defaultValue('config_host_' + key, text);
	}

	function isIpLiteral(value)
	{
		// IPv6 may end in an IPv4 part (::ffff:192.0.2.1) and carry a scope (fe80::1%eth0)
		return /^\d{1,3}(\.\d{1,3}){3}$/.test(value) ||
			(/^[0-9a-f:]+(\d{1,3}(\.\d{1,3}){3})?(%[0-9a-z_.\-]+)?$/i.test(value) && value.indexOf(':') !== -1);
	}

	function enhanceHost(option, input)
	{
		if (input.siblings('.config-host-lookup').length)
		{
			return;
		}
		var box = $('<div class="config-host-lookup" aria-live="polite" hidden></div>').attr('data-multiid', option.multiid);
		input.after(box);
		input.on('input.lookup change.lookup', function(e)
		{
			scheduleHostLookup(box, e.type === 'change' ? 0 : 700, e.type === 'change');
			scheduleBackboneSuggestion(this, e.type === 'change' ? 0 : 700, true);
		});
	}

	/*** SERVER BACKBONE ***********************************************************/

	// Servers on one backbone read from one article store: a 430 from one of them
	// means the others are not asked. The menu lists the bundled backbones
	// (rust/data/backbones.yaml, read by the daemon), "Other" for a name of your
	// own, and "Independent". The host name suggests a backbone: a confirmed news
	// host fills it in (unless one was picked by hand), a provider's domain only
	// offers its backbones to click, and an unknown host gets a warning.
	var BACKBONE_CUSTOM = '\u0001custom';
	var backboneList = null;
	var backboneListWaiters = [];

	function backboneText(key, text)
	{
		return I18n.defaultValue('config_backbone_' + key, text);
	}

	function withBackboneList(done)
	{
		if (backboneList)
		{
			done(backboneList);
			return;
		}
		backboneListWaiters.push(done);
		if (backboneListWaiters.length > 1)
		{
			return;
		}
		RPC.call('backbones', [''], function(result)
		{
			backboneList = (result && result.Backbones || []).map(function(b) { return b.Name; });
			var waiters = backboneListWaiters;
			backboneListWaiters = [];
			waiters.forEach(function(w) { w(backboneList); });
		}, function()
		{
			backboneList = [];
			var waiters = backboneListWaiters;
			backboneListWaiters = [];
			waiters.forEach(function(w) { w(backboneList); });
		});
	}

	function sameBackbone(a, b)
	{
		var norm = function(v) { return $.trim(String(v || '')).replace(/\s+/g, ' ').toLowerCase(); };
		return norm(a) === norm(b);
	}

	function enhanceBackbone(option, input)
	{
		var wrap = $('<div class="config-backbone"></div>');
		var select = $('<select class="config-select config-backbone__menu"></select>')
			.attr('aria-label', backboneText('label', 'Backbone'));
		var hint = $('<div class="config-backbone__hint" aria-live="polite" hidden></div>');
		input.before(wrap);
		wrap.append(select).append(input).append(hint);
		input.addClass('config-backbone-source').attr('placeholder', backboneText('custom_placeholder', 'Name of the backbone'));
		select.on('change', function()
		{
			if (this.value === BACKBONE_CUSTOM)
			{
				// keep the current name to edit; the menu stays on Other while typing
				input.data('customMode', true).prop('hidden', false).trigger('focus');
			}
			else
			{
				input.removeData('customMode').val(this.value).prop('hidden', true);
			}
			input.removeData('autoValue').trigger('input').trigger('change');
			refreshChangeState();
		});
		input.on('input.backbone change.backbone', function()
		{
			if (!sameBackbone(input.val(), input.data('autoValue')))
			{
				input.removeData('autoValue');
			}
		});
		input.on('blur.backbone', function()
		{
			if (input.data('customMode'))
			{
				input.removeData('customMode');
				syncBackbone(input);
			}
		});
		withBackboneList(function()
		{
			syncBackbone(input);
			var host = findOptionByName('Server' + option.multiid + '.Host');
			if (host)
			{
				scheduleBackboneSuggestion(document.getElementById(host.formId), 0, false);
			}
		});
	}

	function syncBackbone(input)
	{
		var select = input.siblings('select.config-backbone__menu');
		if (!select.length)
		{
			return;
		}
		var value = $.trim(input.val() || '');
		var list = backboneList || [];
		select.empty();
		select.append($('<option value=""></option>').text(backboneText('independent', 'Independent (shares articles with no other server)')));
		list.forEach(function(name) { select.append($('<option></option>').attr('value', name).text(name)); });
		var known = !value || list.some(function(name) { return sameBackbone(name, value); });
		if (value && !known)
		{
			select.append($('<option class="config-backbone__own"></option>').attr('value', value).text(value));
		}
		select.append($('<option></option>').attr('value', BACKBONE_CUSTOM).text(backboneText('custom', 'Other (type a name)…')));
		if (input.data('customMode'))
		{
			select.val(BACKBONE_CUSTOM);
			input.prop('hidden', false);
			return;
		}
		var match = list.filter(function(name) { return sameBackbone(name, value); })[0];
		select.val(match || value);
		if (!input.is(':focus'))
		{
			input.prop('hidden', true);
		}
	}

	function setBackbone(input, value, auto)
	{
		input.val(value).trigger('input').trigger('change');
		if (auto)
		{
			input.data('autoValue', value);
		}
		syncBackbone(input);
		refreshChangeState();
	}

	// `edited`: the host was just typed, so a confirmed backbone may be filled in. When the
	// page opens it is only offered: an empty Backbone may be a deliberate Independent.
	function scheduleBackboneSuggestion(hostControl, delay, edited)
	{
		var option = hostControl && findOptionById(hostControl.id);
		if (!option)
		{
			return;
		}
		var $host = $(hostControl);
		clearTimeout($host.data('backboneTimer'));
		$host.data('backboneTimer', setTimeout(function() { suggestBackbone(option.multiid, edited); }, delay));
	}

	function suggestBackbone(multiid, edited)
	{
		var hostOption = findOptionByName('Server' + multiid + '.Host');
		var backboneOption = findOptionByName('Server' + multiid + '.Backbone');
		if (!hostOption || !backboneOption)
		{
			return;
		}
		var input = $('#' + backboneOption.formId);
		var hint = input.siblings('.config-backbone__hint');
		var host = $.trim(getOptionValue(hostOption) || '');
		var seq = (input.data('suggestSeq') || 0) + 1;
		input.data('suggestSeq', seq);
		if (!host || /\$\{/.test(host))
		{
			hint.empty().prop('hidden', true);
			return;
		}
		RPC.call('backbones', [host], function(result)
		{
			if (seq !== input.data('suggestSeq'))
			{
				return;
			}
			renderBackboneSuggestion(input, hint, host, result || {}, edited);
		}, function()
		{
			hint.empty().prop('hidden', true);
		});
	}

	function renderBackboneSuggestion(input, hint, host, result, edited)
	{
		var current = $.trim(input.val() || '');
		var filled = input.data('autoValue');
		if (edited && result.Match !== 'confirmed' && filled !== undefined && sameBackbone(current, filled))
		{
			// filled in for the previous host, which this one is not: back to empty
			input.removeData('autoValue');
			setBackbone(input, '', false);
			current = '';
		}
		var matches = result.Matches || [];
		hint.empty().removeAttr('data-state').prop('hidden', true);
		var note = function(state, text)
		{
			hint.attr('data-state', state).prop('hidden', false)
				.append($('<p class="config-backbone__note"></p>').text(text));
		};
		if (result.Match === 'confirmed' && matches.length)
		{
			var auto = input.data('autoValue');
			if (edited && (!current || (auto !== undefined && sameBackbone(current, auto))))
			{
				setBackbone(input, matches[0], true);
				note('ok', host + ' ' + backboneText('reads_from', 'reads from') + ' ' + matches[0] + '. ' +
					backboneText('filled', 'Filled in for you.'));
			}
			else if (!current)
			{
				note('suggest', host + ' ' + backboneText('reads_from', 'reads from') + ' ' + matches[0] + '. ' +
					backboneText('confirm', 'Use it?'));
				addBackboneButtons(input, hint, matches);
			}
			else if (!sameBackbone(current, matches[0]))
			{
				note('warn', host + ' ' + backboneText('reads_from', 'reads from') + ' ' + matches[0] + ', ' +
					backboneText('not_picked', 'not the backbone picked here.'));
				addBackboneButtons(input, hint, matches);
			}
			return;
		}
		if (result.Match === 'suggested' && matches.length)
		{
			if (matches.some(function(name) { return sameBackbone(name, current); }))
			{
				return;
			}
			note('suggest', matches.length === 1 ?
				host + ' ' + backboneText('looks_like', 'looks like') + ' ' + result.Provider + ', ' +
					backboneText('sells_one', 'which sells') + ' ' + matches[0] + '. ' + backboneText('confirm', 'Use it?') :
				result.Provider + ' ' + backboneText('sells_many', 'sells access to several backbones:') + ' ' +
					matches.join(', ') + '. ' + backboneText('pick_plan', 'Pick the one your plan uses.'));
			addBackboneButtons(input, hint, matches);
			return;
		}
		if (result.Match === 'unknown' && !current)
		{
			note('warn', backboneText('unknown', 'Unknown backbone for') + ' ' + host + '. ' +
				backboneText('unknown_text', 'Pick one from the list, or choose Other to type a name. Leave it Independent if no other server shares its articles.'));
		}
	}

	function addBackboneButtons(input, hint, names)
	{
		var row = $('<p class="config-backbone__picks"></p>');
		names.forEach(function(name)
		{
			$('<button type="button" class="btn btn-small config-backbone__pick"></button>')
				.text(backboneText('use', 'Use') + ' ' + name)
				.on('click', function()
				{
					setBackbone(input, name, false);
					hint.empty().prop('hidden', true);
				})
				.appendTo(row);
		});
		hint.append(row);
	}

	// A new port means new connect times: look the host up again.
	function enhanceHostPort(input)
	{
		input.off('.lookup').on('input.lookup change.lookup', function(e)
		{
			var option = findOptionById(this.id);
			var host = option && findOptionByName('Server' + option.multiid + '.Host');
			var box = host ? $('#' + host.formId).siblings('.config-host-lookup') : $();
			if (box.length && !box.prop('hidden'))
			{
				scheduleHostLookup(box, e.type === 'change' ? 0 : 700, true);
			}
		});
	}

	// Any edit makes a pending reply stale at once, so its addresses can't be picked
	// during the pause before the next lookup starts.
	// A change event that only confirms what was already looked up (leaving the field
	// to click an address) changes nothing.
	// `committed`: the field was left (a change event), so a one-word name such as
	// localhost is the whole name and not the start of a longer one being typed.
	function scheduleHostLookup(box, delay, committed)
	{
		if (box.data('key') === hostLookupKey(box) && box.data('seq') === box.data('lookupSeq'))
		{
			clearTimeout(box.data('timer'));
			return;
		}
		box.data('seq', ++hostLookupSeq);
		box.find('input[type=radio]').prop('disabled', true);
		clearTimeout(box.data('timer'));
		box.data('timer', setTimeout(function() { lookupHost(box, committed); }, delay));
	}

	function hostLookupKey(box)
	{
		var multiid = parseInt(box.attr('data-multiid'), 10);
		var hostOption = findOptionByName('Server' + multiid + '.Host');
		var portOption = findOptionByName('Server' + multiid + '.Port');
		return hostOption ? $.trim(getOptionValue(hostOption) || '') + ' ' + (portOption ? getOptionValue(portOption) : '') : '';
	}

	function lookupHost(box, committed)
	{
		box.data('key', hostLookupKey(box));
		// the last reply belongs to the last host: an IP version switch before the
		// next reply must not show it again
		box.removeData('result').removeData('resultHost');
		var multiid = parseInt(box.attr('data-multiid'), 10);
		var hostOption = findOptionByName('Server' + multiid + '.Host');
		var portOption = findOptionByName('Server' + multiid + '.Port');
		if (!hostOption || !box.closest('body').length)
		{
			return;
		}
		var host = $.trim(getOptionValue(hostOption) || '');
		if (host && !isIpLiteral(host) && box.data('certWasStrict'))
		{
			// a name typed back in is checked strictly again, unless the check was changed by hand since
			var cert = findOptionByName('Server' + multiid + '.CertVerification');
			if (cert && String(getOptionValue(cert)).toLowerCase() === 'minimal')
			{
				setOptionValue(cert, 'Strict');
				refreshChangeState();
				refreshListRows();
			}
			box.removeData('certWasStrict');
		}
		if (!host || /\$\{/.test(host) || (!committed && !isIpLiteral(host) && host.indexOf('.') === -1))
		{
			box.empty().prop('hidden', true);
			return;
		}
		if (!isIpLiteral(host))
		{
			box.data('hostName', host);
		}
		var seq = ++hostLookupSeq;
		box.data('seq', seq).data('lookupSeq', seq);
		box.prop('hidden', false).attr('data-state', 'busy').empty()
			.append($('<p class="config-host-lookup__note"></p>').text(hostText('looking', 'Looking up') + ' ' + host + '…'));
		var port = parseInt(portOption ? getOptionValue(portOption) : '', 10) || 563;
		RPC.call('hostlookup', [host, port], function(result)
		{
			if (seq !== box.data('seq'))
			{
				return;
			}
			renderHostLookup(box, host, result);
		}, function(value, result)
		{
			if (seq !== box.data('seq'))
			{
				return;
			}
			box.attr('data-state', 'error').empty().append($('<p class="config-host-lookup__note"></p>')
				.text(result && result.error ? result.error.message : String(value)));
		}, { timeout: 20000 });
	}

	// IP version changed: list again the addresses the server can use now
	function rerenderHostLookup(option)
	{
		var host = findOptionByName('Server' + option.multiid + '.Host');
		var box = host ? $('#' + host.formId).siblings('.config-host-lookup') : $();
		if (box.length && !box.prop('hidden') && box.data('result') && box.data('seq') === box.data('lookupSeq'))
		{
			renderHostLookup(box, box.data('resultHost'), box.data('result'));
		}
	}

	function addressOption(box)
	{
		return findOptionByName('Server' + parseInt(box.attr('data-multiid'), 10) + '.Address');
	}

	function renderHostLookup(box, host, result)
	{
		box.data('result', result).data('resultHost', host);
		box.empty().attr('data-state', result.Error ? 'error' : 'ok');
		if (result.Error)
		{
			box.append($('<p class="config-host-lookup__note"></p>').text(result.Error));
			return;
		}
		var name = box.data('hostName');
		// an address typed as Host (the old way to pin one) still works, without the name for TLS
		var literal = isIpLiteral(host);
		var addrOption = addressOption(box);
		var current = literal ? host : $.trim(addrOption ? getOptionValue(addrOption) || '' : '');
		// a server set to IPv4 or IPv6 only can't use an address of the other kind
		var family = serverIpVersion(box);
		var addresses = (result.Addresses || []).filter(function(address)
		{
			return !family || ipFamily(address.IP) === family || address.IP === current;
		});
		var head = $('<p class="config-host-lookup__head"></p>');
		head.text(literal ? hostText('literal', 'Host is an address') + (name ? ' (' + name + ')' : '') :
			addresses.length === 1 ? hostText('one', '1 address for') + ' ' + host :
			addresses.length + ' ' + hostText('many', 'addresses for') + ' ' + host);
		box.append(head);
		var list = $('<div class="config-host-lookup__list" role="radiogroup"></div>').attr('aria-label', head.text());
		var group = 'ConfigHostPick' + (++hostLookupSeq);
		var choice = function(value, checked, main, details)
		{
			var label = $('<label class="config-host-lookup__item"><input type="radio"><span class="config-host-lookup__main"></span>' +
				'<span class="config-host-lookup__details"></span><span class="config-host-lookup__check" hidden></span></label>');
			label.attr('data-ip', value);
			label.find('input').attr({ name: group, value: value }).prop('checked', checked);
			label.find('.config-host-lookup__main').text(main);
			label.find('.config-host-lookup__details').text(details || '');
			list.append(label);
		};
		if (literal)
		{
			if (!name)
			{
				box.append($('<p class="config-host-lookup__note"></p>').text(hostText('literal_text',
					'Type the host name instead and pick this address below: the name is then kept for the certificate check.')));
			}
			if (name)
			{
				choice('\u0001name', false, hostText('use_name', 'Use the name again') + ': ' + name,
					hostText('use_name_text', 'Keeps this address as the first to try, and the name for the certificate check.'));
			}
		}
		else
		{
			choice('', !current, hostText('auto', 'Auto: the fastest (recommended)'),
				hostText('auto_text', 'RustyGet times each address, uses the fastest, and follows the provider if the addresses change.'));
		}
		var listed = false;
		addresses.forEach(function(address)
		{
			var where = [];
			if (address.Country) where.push(address.Country + (address.CountryName ? ' ' + address.CountryName : ''));
			if (address.ASN) where.push('AS' + address.ASN + (address.Org ? ' ' + address.Org : ''));
			where.push(address.LatencyMs === null || address.LatencyMs === undefined ? hostText('no_answer', 'no answer') :
				Math.round(address.LatencyMs) + ' ms');
			listed = listed || address.IP === current;
			choice(address.IP, address.IP === current, address.IP, where.join(' · '));
		});
		if (current && !listed && !literal)
		{
			choice(current, true, current, hostText('gone', 'Not an address of this host now: skipped unless the certificate is checked strictly.'));
		}
		if (list.children().length)
		{
			box.append(list);
		}
		list.on('change', 'input', function() { pickHostAddress(box, this.value); });
		if (!literal && addresses.length)
		{
			$('<p class="config-host-lookup__actions"></p>').append(
				$('<button type="button" class="btn btn-small config-host-lookup__login"></button>')
					.text(hostText('check_login', 'Check login on each address'))
					.on('click', function() { checkAddressLogins(box, this); }))
				.appendTo(box);
		}
		box.append($('<p class="config-host-lookup__source"></p>').append(result.GeoIP ?
			$('<a href="https://db-ip.com" target="_blank" rel="noopener"></a>').text(hostText('source', 'IP location data by DB-IP')) :
			$('<span></span>').text(hostText('no_geoip', 'Country and network need the DB-IP Lite files, which this install does not have.'))));
	}

	function ipFamily(ip)
	{
		return String(ip).indexOf(':') !== -1 ? 'ipv6' : 'ipv4';
	}

	// 'ipv4' or 'ipv6' when the server is limited to one, else ''
	function serverIpVersion(box)
	{
		var option = findOptionByName('Server' + parseInt(box.attr('data-multiid'), 10) + '.IpVersion');
		var value = option ? String(getOptionValue(option) || '').toLowerCase() : '';
		return value === 'ipv4' || value === 'ipv6' ? value : '';
	}

	// Picking an address writes it as Address (empty is Auto); Host keeps the name, so the
	// certificate check is unchanged. On a server whose Host is an address (the old way to
	// pin one), "Use the name again" moves that address to Address and the name back to Host.
	function pickHostAddress(box, ip)
	{
		var multiid = parseInt(box.attr('data-multiid'), 10);
		var hostOption = findOptionByName('Server' + multiid + '.Host');
		var addrOption = addressOption(box);
		if (!hostOption || !addrOption)
		{
			return;
		}
		var oldHost = $.trim(getOptionValue(hostOption) || '');
		if (ip === '\u0001name' || (ip && isIpLiteral(oldHost)))
		{
			// Address is not used while Host is an address: picking one moves the name back to
			// Host and the pick to Address, or, with no name known, writes the pick as Host
			var name = box.data('hostName');
			var pick = ip === '\u0001name' ? oldHost : ip;
			if (!name && ip === '\u0001name')
			{
				return;
			}
			$('#' + hostOption.formId).val(name || ip);
			if (!name)
			{
				pick = '';
			}
			setOptionValue(addrOption, isIpLiteral(pick) ? pick : '');
			var cert = findOptionByName('Server' + multiid + '.CertVerification');
			if (name && cert && box.data('certWasStrict') && String(getOptionValue(cert)).toLowerCase() === 'minimal')
			{
				setOptionValue(cert, 'Strict');
			}
			box.removeData('certWasStrict');
		}
		else
		{
			setOptionValue(addrOption, ip);
			var family = serverIpVersion(box);
			var ipVersion = findOptionByName('Server' + multiid + '.IpVersion');
			if (ip && family && ipFamily(ip) !== family && ipVersion)
			{
				// an address of the other kind (a list shown before IP version was changed) needs Automatic
				setOptionValue(ipVersion, 'auto');
			}
		}
		refreshChangeState();
		refreshListRows();
		clearTimeout(box.data('timer'));
		lookupHost(box, true);
	}

	// Logs in on each listed address, one at a time (each check sends QUIT when done), with
	// the server's current username, password and encryption settings.
	function checkAddressLogins(box, button)
	{
		var multiid = parseInt(box.attr('data-multiid'), 10);
		var value = function(field)
		{
			var option = findOptionByName('Server' + multiid + '.' + field);
			return option ? getOptionValue(option) : '';
		};
		var host = $.trim(value('Host') || '');
		var rows = box.find('.config-host-lookup__item').filter(function() { return isIpLiteral($(this).attr('data-ip') || ''); }).toArray();
		var seq = box.data('seq');
		var timeout = Math.min(parseInt(getOptionValue(findOptionByName('ArticleTimeout')), 10) || 10, 10);
		$(button).prop('disabled', true);
		var next = function()
		{
			var row = $(rows.shift());
			if (!row.length || seq !== box.data('seq'))
			{
				$(button).prop('disabled', false);
				return;
			}
			var mark = row.find('.config-host-lookup__check').prop('hidden', false)
				.attr('data-state', 'busy').text(hostText('checking', 'checking…'));
			var done = function(state, text)
			{
				if (seq === box.data('seq'))
				{
					mark.attr('data-state', state).text(text);
				}
				next();
			};
			RPC.call('testserver', [host, parseInt(value('Port'), 10), value('Username'), value('Password'),
				String(value('Encryption')).toLowerCase() === 'yes', value('Cipher'), timeout,
				getCertVerifLevel(value('CertVerification')), row.attr('data-ip')],
				function(errtext)
				{
					if (errtext === '') done('ok', hostText('login_ok', 'login OK'));
					else done('error', String(errtext));
				},
				function(message, result)
				{
					done('error', result && result.error && result.error.message ? String(result.error.message) : String(message));
				}, { timeout: (timeout + 5) * 1000 });
		};
		next();
	}

	/*** RSS FEEDS ********************************************************************/

	this.editFilter = function(optFormId)
	{
		var option = findOptionById(optFormId);
		FeedFilterDialog.showModal(
			option.multiid,
			getOptionValue(findOptionByName('Feed' + option.multiid + '.Name')),
			getOptionValue(findOptionByName('Feed' + option.multiid + '.URL')),
			getOptionValue(findOptionByName('Feed' + option.multiid + '.Filter')),
			getOptionValue(findOptionByName('Feed' + option.multiid + '.Backlog')),
			getOptionValue(findOptionByName('Feed' + option.multiid + '.PauseNzb')),
			getOptionValue(findOptionByName('Feed' + option.multiid + '.Category')),
			getOptionValue(findOptionByName('Feed' + option.multiid + '.Priority')),
			getOptionValue(findOptionByName('Feed' + option.multiid + '.Interval')),
			getOptionValue(findOptionByName('Feed' + option.multiid + '.CategorySource')),
			function(filter)
				{
					var control = $('#' + option.formId);
					control.val(filter);
					refreshChangeState();
				});
	}

	this.previewFeed = function(control, setname, sectionId)
	{
		var multiid = parseInt($(control).attr('data-multiid'));
		FeedDialog.showModal(multiid,
			getOptionValue(findOptionByName('Feed' + multiid + '.Name')),
			getOptionValue(findOptionByName('Feed' + multiid + '.URL')),
			getOptionValue(findOptionByName('Feed' + multiid + '.Filter')),
			getOptionValue(findOptionByName('Feed' + multiid + '.Backlog')),
			getOptionValue(findOptionByName('Feed' + multiid + '.PauseNzb')),
			getOptionValue(findOptionByName('Feed' + multiid + '.Category')),
			getOptionValue(findOptionByName('Feed' + multiid + '.Priority')),
			getOptionValue(findOptionByName('Feed' + multiid + '.Interval')),
			getOptionValue(findOptionByName('Feed' + multiid + '.CategorySource')),
		);
	}

	/*** TEST SERVER ********************************************************************/

	// Captured only in this browser; never transmit credentials in job status.
	this.cipherSnapshot = function()
	{
		if (!config) return null;
		var section = findSectionById('NEWS-SERVERS');
		if (!section || section.modified) return null;
		var values = [], changed = false;
		section.options.forEach(function(option) {
			if (option.template || !/^Server\d+\./i.test(option.name)) return;
			var current = String(getOptionValue(option) || '');
			var saved = String((option.value === null ? option.defvalue : option.value) || '');
			// Switch tokens are case-insensitive daemon enums. The editor's
			// canonical "Strict" token can represent the saved value "strict".
			if (option.type === 'switch') { current = current.toLowerCase(); saved = saved.toLowerCase(); }
			if (current !== saved) changed = true;
			values.push([option.name, current]);
		});
		values.sort(function(a, b) { return a[0].localeCompare(b[0]); });
		return changed ? null : JSON.stringify(values);
	};

	this.applyCipherResults = function(result, snapshot)
	{
		var currentSnapshot = Config.cipherSnapshot();
		if (currentSnapshot === null) return false;
		var canPatch = snapshot && currentSnapshot === snapshot;
		var updates = [];
		for (var i = 0; i < (result.Servers || []).length; i++)
		{
			var server = result.Servers[i];
			if (server.Outcome !== 'saved') continue;
			if (!Number.isInteger(server.ID) || server.ID <= 0 || !/^[A-Za-z0-9_-]+$/.test(server.SelectedCipher || '')) return false;
			var prefix = 'Server' + server.ID + '.';
			var cipher = findOptionByName(prefix + 'Cipher');
			var host = findOptionByName(prefix + 'Host'), port = findOptionByName(prefix + 'Port');
			// The editor holds the saved text. A ${...} reference or a zero-padded
			// port differs from the resolved Host and Port the daemon probed.
			if (!cipher || !host || !port || typeof server.SavedHost !== 'string' || typeof server.SavedPort !== 'string' ||
				getOptionValue(host) !== server.SavedHost || String(getOptionValue(port)) !== server.SavedPort) return false;
			// A fresh Settings read may already contain the persisted result.
			// This no-op needs no old snapshot; changing a value still does.
			if (getOptionValue(cipher) === server.SelectedCipher) continue;
			if (!canPatch || getOptionValue(cipher) !== server.OldCipher) return false;
			updates.push({ option: cipher, value: server.SelectedCipher });
		}
		updates.forEach(function(update) {
			setOptionValue(update.option, update.value);
			syncCipher($('#' + update.option.formId).siblings('select.config-cipher__menu'));
			update.option.value = update.value;
			var saved = (config.values || []).filter(function(value) { return value.Name.toLowerCase() === update.option.name.toLowerCase(); });
			if (saved.length) saved.forEach(function(value) { value.Value = update.value; });
			else if (config.values) config.values.push({ Name: update.option.name, Value: update.value });
		});
		refreshListRows();
		return true;
	};

	// Compare saved server identities/ciphers under the daemon's write lock.
	// Never use editor values here: intentional edits belong in the save payload.
	this.cipherBaseline = function(values)
	{
		var fields = Object.create(null), encoder = new TextEncoder();
		(values || config.values || []).forEach(function(option) {
			if (!/^Server[1-9][0-9]*\.(Host|Port|Cipher)$/i.test(option.Name)) return;
			var bytes = encoder.encode(option.Value), hex = '';
			bytes.forEach(function(value) { hex += ('0' + value.toString(16)).slice(-2); });
			fields[option.Name.toLowerCase()] = hex;
		});
		return 'v1\n' + Object.keys(fields).sort().map(function(name) { return name + '=' + fields[name] + '\n'; }).join('');
	};
	this.withCipherBaseline = function(values)
	{
		return values.concat([{ Name: '_TlsCipherBaseline', Value: Config.cipherBaseline() }]);
	};

	var connecting = false;

	this.testConnection = function(control, setname, sectionId)
	{
		if (connecting)
		{
			return;
		}

		connecting = true;
		$('#Notif_Config_TestConnectionProgress').fadeIn(function() {
			var multiid = parseInt($(control).attr('data-multiid'));
			var timeout = Math.min(parseInt(getOptionValue(findOptionByName('ArticleTimeout'))), 10);
			var certVerifLevel = getCertVerifLevel(getOptionValue(findOptionByName('Server' + multiid + '.CertVerification')));
			RPC.call('testserver', [
				getOptionValue(findOptionByName('Server' + multiid + '.Host')),
				parseInt(getOptionValue(findOptionByName('Server' + multiid + '.Port'))),
				getOptionValue(findOptionByName('Server' + multiid + '.Username')),
				getOptionValue(findOptionByName('Server' + multiid + '.Password')),
				getOptionValue(findOptionByName('Server' + multiid + '.Encryption')) === 'yes',
				getOptionValue(findOptionByName('Server' + multiid + '.Cipher')),
				timeout,
				certVerifLevel
				],
				function(errtext) {
					$('#Notif_Config_TestConnectionProgress').fadeOut(function() {
						if (errtext == '')
						{
							PopupNotification.show('#Notif_Config_TestConnectionOK');
						}
						else
						{
							// the news server's reply: text, not markup
							AlertDialog.showModal(I18n.translate('msg_connection_test_failed'), Util.textToHtml(errtext));
						}
					});
					connecting = false;
				},
				function(message, resultObj) {
					$('#Notif_Config_TestConnectionProgress').fadeOut(function() {
						if (resultObj && resultObj.error && resultObj.error.message)
						{
							message = Util.textToHtml(String(resultObj.error.message));
						}
						AlertDialog.showModal(I18n.translate('msg_connection_test_failed'), message);
						connecting = false;
					});
				});
		});
	}

	/*** DOWNLOADED VOLUMES FOR A SERVER *******************************************************/

	this.serverStats = function(control, setname, sectionId)
	{
		var multiid = parseInt($(control).attr('data-multiid'));

		// Because the settings page isn't saved yet and user can reorder servers
		// we cannot just use the server-id in teh call to "StatDialog".
		// Instead we need to find the server in loaded options and get its ID from there.

		var serverName = getOptionValue(findOptionByName('Server' + multiid + '.Name'));
		var serverHost = getOptionValue(findOptionByName('Server' + multiid + '.Host'));
		var serverPort = getOptionValue(findOptionByName('Server' + multiid + '.Port'));
		var serverId = 0;

		// searching by name
		var opt = undefined;
		for (var i = 1; opt !== null; i++)
		{
			var opt = Options.option('Server' + i + '.Name');
			if (opt === serverName)
			{
				serverId = i;
				break;
			}
		}
		if (serverId === 0)
		{
			// searching by host:port
			var host = undefined;
			for (var i = 1; host !== null; i++)
			{
				host = Options.option('Server' + i + '.Host');
				var port = Options.option('Server' + i + '.Port');
				if (host === serverHost && port === serverPort)
				{
					serverId = i;
					break;
				}
			}
		}

		if (serverId === 0)
		{
			AlertDialog.showModal(I18n.translate('title_downloaded_volumes'), I18n.translate('msg_no_stats_available'));
			return;
		}

		StatDialog.showModal(serverId);
	}

	/*** SAVE ********************************************************************/

	function getOptionValue(option)
	{
		var control = $('#' + option.formId);
		if (option.type === 'switch')
		{
			return switchGetValue(control);
		}
		else
		{
			return control.val();
		}
	}
	this.getOptionValue = getOptionValue;

	function setOptionValue(option, value)
	{
		var control = $('#' + option.formId);
		if (option.type === 'switch')
		{
			switchSetValue(control, value);
		}
		else
		{
			control.val(value);
		}
	}
	this.setOptionValue = setOptionValue;

	// Checks if there are obsolete or invalid options
	function invalidOptionsExist()
	{
		var hiddenOptions = ['ConfigFile', 'AppBin', 'AppDir', 'Version'];

		for (var i=0; i < Options.options.length; i++)
		{
			var option = Options.options[i];
			var confOpt = findOptionByName(option.Name);
			if (!confOpt && hiddenOptions.indexOf(option.Name) === -1)
			{
				return true;
			}
		}

		return false;
	}

	// Enum values are matched case-insensitively in the UI: compare the same way.
	function isSameOptionValue(option, value1, value2)
	{
		if (option.select && option.select.length > 1 &&
			typeof value1 === 'string' && typeof value2 === 'string')
		{
			return value1.toLowerCase() === value2.toLowerCase();
		}
		return value1 == value2;
	}

	function prepareSaveRequest(onlyUserChanges, webSettings, onlyChangedOptions)
	{
		var modified = false;
		var request = [];
		for (var k = (webSettings ? 0 : 1); k < (webSettings ? 1 : config.length); k++)
		{
			var sections = config[k].sections;
			for (var i=0; i < sections.length; i++)
			{
				var section = sections[i];
				if (!section.postparam)
				{
					for (var j=0; j < section.options.length; j++)
					{
						var option = section.options[j];
						if (!option.template && !(option.type === 'info') && !option.commandopts)
						{
							var oldValue = option.value;
							var newValue = getOptionValue(option);
							if (section.hidden)
							{
								newValue = oldValue;
							}
							if (newValue != null)
							{
								if (onlyUserChanges)
								{
									var optmodified = !isSameOptionValue(option, oldValue, newValue) && oldValue !== null;
								}
								else
								{
									var optmodified = !isSameOptionValue(option, oldValue, newValue) || (option.value === null);
								}
								modified = modified || optmodified;
								if (optmodified || !onlyChangedOptions)
								{
									var opt = {Name: option.name, Value: newValue};
									request.push(opt);
								}
							}
						}
						modified = modified || section.modified;
					}
				}
			}
		}

		return modified || (!onlyUserChanges && invalidOptionsExist()) || restored ? request : [];
	}

	this.saveChanges = function()
	{
		$LeaveConfigDialog.modal('hide');
		if (serverSaving)
		{
			setServerStatus(serverText('save_wait', 'A server is still being saved. Save the other changes once it is done.'), 'busy');
			return;
		}
		if (typeof TlsCipherDialog !== 'undefined' && !TlsCipherDialog.canSaveSettings())
		{
			TlsCipherDialog.showSaveBlocked();
			return;
		}

		refreshValidation();
		if (invalidOptions.length > 0)
		{
			focusFirstInvalid();
			return;
		}

		var serverSaveRequest = prepareSaveRequest(false, false);
		var webSaveRequest = prepareSaveRequest(false, true);

		if (serverSaveRequest.length === 0 && webSaveRequest.length === 0)
		{
			PopupNotification.show('#Notif_Config_Unchanged');
			return;
		}

		showSaveBanner();

		Util.show('#ConfigSaved_Reload, #ConfigReload', serverSaveRequest.length > 0);

		if (webSaveRequest.length > 0)
		{
			saveWebSettings(webSaveRequest);
		}

		if (serverSaveRequest.length > 0)
		{
			$('#Notif_Config_Failed_Filename').text(Options.option('ConfigFile'));
			// lines of the file a save leaves out (they are not settings) are
			// listed first, and removed only when the user agrees
			RPC.call('checkconfig', [serverSaveRequest], function(problems)
			{
				var dropped = (problems || []).filter(function(p) { return p.Name === ''; });
				if (!dropped.length)
				{
					sendSave(serverSaveRequest);
					return;
				}
				var confirmed = false;
				ConfirmDialog.showModal('DropLinesConfirmDialog', function()
				{
					confirmed = true;
					sendSave(serverSaveRequest);
				}, function(dialog)
				{
					$('#ConfirmDialog_Text', dialog).append('<ul class="config-drop-list">' + dropped.map(function(p)
					{
						return '<li>' + Util.textToHtml(p.Problem) + '</li>';
					}).join('') + '</ul>');
					dialog.one('hidden', function() { if (!confirmed) removeSaveBanner(); });
				});
			}, function()
			{
				// an older daemon without checkconfig
				sendSave(serverSaveRequest);
			});
		}
		else
		{
			// only web-settings were changed, refresh page
			document.location.reload(true);
		}
	}

	function sendSave(serverSaveRequest)
	{
		{
			RPC.call('saveconfig', [Config.withCipherBaseline(serverSaveRequest)], saveCompleted, function(value, result) {
				removeSaveBanner();
				var text = result && result.error ? result.error.message : String(value);
				if (result && result.error && result.error.code === 3)
				{
					$('#Notif_Config_CipherPending strong').text(text);
					PopupNotification.show('#Notif_Config_CipherPending');
				}
				else
				{
					// the daemon refused the values or could not write: say why, nothing was changed
					AlertDialog.showModal(I18n.defaultValue('config_not_saved', 'Settings were not saved'), Util.textToHtml(text));
				}
			});
		}
	}

	function getCertVerifLevel(verifLevel)
	{
		var level = verifLevel.toLowerCase();
		switch(level)
		{
			case "none": return 0;
			case "minimal": return 1;
			case "strict": return 2;
			default: return 2;
		}
	}

	function showSaveBanner()
	{
		pageSaving = true;
		$('#Config_Save').attr('disabled', 'disabled');
		$('.config-server-save, .config-cipher__optimize', $ConfigData).prop('disabled', true);
	}

	function removeSaveBanner()
	{
		pageSaving = false;
		$('#Config_Save').removeAttr('disabled');
		$('.config-server-save, .config-cipher__optimize', $ConfigData).prop('disabled', serverSaving);
		refreshValidation();
	}

	function saveCompleted(result)
	{
		removeSaveBanner();
		if (result)
		{
			$ConfigContent.fadeOut(function() { $('#ConfigSaved').fadeIn(); });
			// The values are saved now: stop forcing a full save on every leave-tab check.
			restored = false;
		}
		else
		{
			PopupNotification.show('#Notif_Config_Failed');
		}
		configSaved = true;
	}

	function saveWebSettings(values)
	{
		for (var i=0; i < values.length; i++)
		{
			var option = values[i];
			var optname = option.Name;
			var optvalue = option.Value;
			optname = optname[0].toLowerCase() + optname.substring(1);
			if (optvalue === 'yes') optvalue = true;
			if (optvalue === 'no') optvalue = false;
			UISettings[optname] = optvalue;
		}
		UISettings.save();
	}

	this.canLeaveTab = function(target)
	{
		if (!config || prepareSaveRequest(true).length === 0 || configSaved)
		{
			return true;
		}

		leaveTarget = target;
		$LeaveConfigDialog.modal({backdrop: 'static'});
		return false;
	}

	function userLeavesPage(e)
	{
		if (config && !configSaved && !UISettings.connectionError && prepareSaveRequest(true).length > 0)
		{
			return I18n.translate('msg_discard_changes_question');
		}
	}

	this.discardChanges = function()
	{
		configSaved = true;
		restored = false;
		$LeaveConfigDialog.modal('hide');
		leaveTarget.click();
	}

	this.scrollToOption = function(event, control)
	{
		event.preventDefault();

		if ($(control).hasClass('option-name') && !$ConfigContent.hasClass('search'))
		{
			// Click on option title scrolls only from Search-page, not from regual pages
			return;
		}

		var optid = $(control).attr('data-optid');
		if (!optid)
		{
			var optname = $(control).text();
			var option = findOptionByName(optname);
			if (option)
			{
				optid = option.formId;
			}
		}
		if (optid)
		{
			scrollOptionIntoView(optid);
		}
	}

	this.showSpoiler = function(control)
	{
		$(control).hide();
		$(control).next().show();
	}

	function filterInput(value)
	{
		filterText = value;
		if (filterText.trim() !== '')
		{
			$('.ConfigSearch').show();
			Config.showSection('Search', true);
		}
		else
		{
			filterClear();
		}
	}

	function filterClear()
	{
		filterText = '';
		Config.showSection(lastSection, true);
		$('.ConfigSearch').hide();
		$ConfigTabBadge.hide();
		$ConfigTabBadgeEmpty.show();
	}

	var searcher = new FastSearcher();

	function search()
	{
		closeServerEditor();
		$ConfigTabBadge.show();
		$ConfigTabBadgeEmpty.hide();
		$ConfigContent.addClass('search');

		$ConfigData.children().hide();
		$('.config-card .control-group', $ConfigData).hide();

		searcher.compile(filterText);

		var total = 0;
		var available = 0;

		for (var k=0; k < config.length; k++)
		{
			var sections = config[k].sections;
			for (var i=0; i < sections.length; i++)
			{
				var section = sections[i];
				if (!section.hidden)
				{
					for (var j=0; j < section.options.length; j++)
					{
						var option = section.options[j];
						if (!option.template)
						{
							total++;
							if (filterOption(option))
							{
								available++;
								var opt = $('#' + option.formId).closest('.control-group');
								showOptionRows(opt);
								opt.closest('.config-card').show();
								opt.closest('details.config-card__more').prop('open', true);
							}
						}
					}
				}
			}
		}

		filterStaticPages();

		markLastControlGroup();

		$ConfigTitle.attr('data-i18n', 'search_results');
		I18n.translatePage($ConfigTitle.parent());
		$Body.animate({ scrollTop: 0 }, { duration: 0 });

		updateTabInfo($ConfigTabBadge, { filter: true, available: available, total: total});
	}

	function filterOption(option)
	{
		return searcher.exec({ name: option.caption + (option.label ? ' ' + option.label : ''), description: option.description, value: (option.value === null ? '' : option.value), _search: ['name', 'description', 'value'] });
	}

	function filterStaticPages()
	{
		$ConfigData.children().filter('.config-static').each(function(index, element)
			{
				var name = $('.control-label', element).text();
				var description = $('.controls', element).text();
				var found = searcher.exec({ name: name, description: description, value: '', _search: ['name', 'description'] });
				Util.show(element, found);
			});
	}

	function markLastControlGroup()
	{
		$ConfigData.children().removeClass('last-group first-group');
		var visible = $ConfigData.children().filter(':visible');
		visible.last().addClass('last-group');
		visible.first().addClass('first-group');
	}

	/*** RELOAD ********************************************************************/

	function restart(callback)
	{
		Refresher.pause();
		$('#ConfigReloadInfoNotes').hide();

		$('body').fadeOut(function()
		{
			$('#Navbar, #MainContent').hide();
			$('#ConfigSaved').hide();
			$('body').toggleClass('navfixed', false);
			$('body').show();
			$('#ConfigReloadInfo').fadeIn();
			reloadTime = new Date();
			callback();
		});
	}

	this.reloadConfirm = function()
	{
		ConfirmDialog.showModal('ReloadConfirmDialog', Config.reload);
	}

	var reloadInstance;

	this.reload = function()
	{
		// the daemon's instance number (setupstatus) tells a restarted daemon apart
		// for sure; the uptime comparison below is the fallback
		var st = typeof Setup !== 'undefined' ? Setup.status() : null;
		reloadInstance = st && st.Instance !== undefined ? st.Instance : undefined;
		$('#ConfigReloadAction').text(I18n.translate('msg_stopping_reloading'));
		restart(function() { RPC.call('reload', [], reloadCheckStatus); });
	}

	var reloadSawDown = false;

	function reloadCheckStatus()
	{
		if (reloadInstance !== undefined)
		{
			RPC.call('setupstatus', [], function(st)
			{
				if (st && st.Instance !== undefined && st.Instance !== reloadInstance)
				{
					$('#ConfigReloadAction').text(I18n.translate('msg_reloaded_refreshing'));
					document.location.reload(true);
				}
				else
				{
					setTimeout(reloadCheckStatus, 500);
					reloadCheckNotes();
				}
			}, function()
			{
				setTimeout(reloadCheckStatus, 500);
				reloadCheckNotes();
			});
			return;
		}
		RPC.call('status', [], function(status)
			{
				// OK, checking if it is a restarted instance (a shorter uptime, or
				// any answer once the port was closed: a reload right after
				// another one starts with the same uptime of 0)
				if (!reloadSawDown && status.UpTimeSec >= Status.status.UpTimeSec)
				{
					// the old instance is not restarted yet
					// waiting 0.5 sec. and retrying
					setTimeout(reloadCheckStatus, 500);
					reloadCheckNotes();
				}
				else
				{
					// restarted successfully
					$('#ConfigReloadAction').text(I18n.translate('msg_reloaded_refreshing'));
					// refresh page
					document.location.reload(true);
				}
			},
			function()
			{
				// Failure, waiting 0.5 sec. and retrying
				reloadSawDown = true;
				setTimeout(reloadCheckStatus, 500);
				reloadCheckNotes();
			});
	}

	function reloadCheckNotes()
	{
		// if reload takes more than 30 sec. show additional tips
		if (new Date() - reloadTime > 30000)
		{
			$('#ConfigReloadInfoNotes').show(1000);
		}
	}

	this.applyReloadedValues = function(values)
	{
		restored = true;
		Options.reloadConfig(values, Config.buildPage);
		refreshChangeState();
	}

	/*** SHUTDOWN ********************************************************************/

	this.shutdownConfirm = function()
	{
		ConfirmDialog.showModal('ShutdownConfirmDialog', Config.shutdown);
	}

	this.shutdown = function()
	{
		$('#ConfigReloadTitle').text(I18n.translate('shutdown_dialog_title'));
		$('#ConfigReloadAction').text(I18n.translate('notif_pausing'));
		restart(function() { RPC.call('shutdown', [], shutdownCheckStatus); });
	}

	function shutdownCheckStatus()
	{
		RPC.call('version', [], function(version)
			{
				// the program still runs, waiting 0.5 sec. and retrying
				setTimeout(shutdownCheckStatus, 500);
			},
			function()
			{
				// the program has been stopped
				$('#ConfigReloadTransmit').hide();
				$('#ConfigReloadAction').text(I18n.translate('msg_program_stopped'));
			});
	}

	/*** UPDATE ********************************************************************/

	this.checkUpdates = function()
	{
		UpdateDialog.showModal();
	}

	this.redraw = function()
	{
		if (config)
		{
			var activeSection = this.currentSectionID || 'Config-Info';
			this.buildPage(config);
			if (activeSection !== 'Config-Info') {
				this.showSection(activeSection, false);
			}
		}
	}
}(jQuery));


/*** BACKUP/RESTORE SETTINGS *******************************************************/

var ConfigBackupRestore = (new function($)
{
	'use strict'

	// State
	var settings;
	var filename;

	this.init = function(options)
	{
		var restoreInput = $('#Config_RestoreInput')[0];
		if (restoreInput)
		{
			restoreInput.addEventListener('change', restoreSelectHandler, false);
		}
	}

	/*** BACKUP ********************************************************************/

	this.backupSettings = function()
	{
		var settings = '';
		for (var i=0; i < Config.config().values.length; i++)
		{
			var option = Config.config().values[i];
			if (option.Value !== null)
			{
				settings += settings==='' ? '' : '\n';
				settings += option.Name + '=' + option.Value;
			}
		}

		var pad = function(arg) { return (arg < 10 ? '0' : '') + arg }
		var dt = new Date();
		var datestr = dt.getFullYear() + pad(dt.getMonth() + 1) + pad(dt.getDate()) + '-' + pad(dt.getHours()) + pad(dt.getMinutes()) + pad(dt.getSeconds());

		var filename = 'rustyget-' + datestr + '.conf';

		if (!Util.saveToLocalFile(settings, "text/plain;charset=utf-8", filename))
		{
			alert(I18n.translate('msg_no_filesystem_access_backup', Options.option('ConfigFile')));
		}
	}

	/*** RESTORE ********************************************************************/

	this.restoreSettings = function()
	{
		if (!window.FileReader)
		{
			alert(I18n.translate('msg_no_filereader_api'));
			return;
		}

		var testreader = new FileReader();
		if (!testreader.readAsBinaryString && !testreader.readAsDataURL)
		{
			alert(I18n.translate('msg_no_direct_file_access'));
			return;
		}

		var inp = $('#Config_RestoreInput');

		// Reset file input control (needed for IE10)
		inp.wrap('<form>').closest('form').get(0).reset();
		inp.unwrap();

		inp.click();
	}

	function restoreSelectHandler(event)
	{
		if (!event.target.files)
		{
			alert(I18n.translate('msg_no_direct_file_access'));
			return;
		}
		if (event.target.files.length > 0)
		{
			restoreFromFile(event.target.files[0]);
		}
	}

	function restoreFromFile(file)
	{
		var reader = new FileReader();
		reader.onload = function (event)
		{
			if (reader.readAsBinaryString)
			{
				settings = event.target.result;
			}
			else
			{
				var base64str = event.target.result.slice(event.target.result.indexOf(',') + 1);
				settings = atob(base64str);
			}
			filename = file.name;

			if (settings.indexOf('MainDir=') < 0)
			{
				alert(I18n.translate('msg_not_valid_backup', filename));
				return;
			}

			RestoreSettingsDialog.showModal(Config.config(), restoreExecute);
		};

		if (reader.readAsBinaryString)
		{
			reader.readAsBinaryString(file);
		}
		else
		{
			reader.readAsDataURL(file);
		}
	}

	function restoreExecute(selectedSections)
	{
		$('#Notif_Config_Restoring').show();
		setTimeout(function() {
			var values = restoreValues(selectedSections);
			Config.applyReloadedValues(values);
			$('#Notif_Config_Restoring').hide();
			$('#SettingsRestoredDialog').modal({backdrop: 'static'});
		}, 50);
	}

	function restoreValues(selectedSections)
	{
		var config = Config.config();
		var values = config.values;

		settings = settings.split('\n');

		for (var i=0; i < settings.length; i++)
		{
			var optstr = settings[i];
			var ind = optstr.indexOf('=');
			var option = { Name: optstr.substr(0, ind).trim(), Value: optstr.substr(ind+1, 100000).trim() };
			settings[i] = option;
		}
		// an older backup has ServerN.Level; read it as the Priority this template knows
		Options.upgradeValues(settings);

		function removeValue(name)
		{
			var name = name.toLowerCase();
			for (var i=0; i < values.length; i++)
			{
				if (values[i].Name.toLowerCase() === name)
				{
					values.splice(i, 1);
					return true;
				}
			}
			return false;
		}

		function addValue(name)
		{
			var name = name.toLowerCase();
			for (var i=0; i < settings.length; i++)
			{
				if (settings[i].Name.toLowerCase() === name)
				{
					values.push(settings[i]);
					return true;
				}
			}
			return false;
		}

		function restoreOption(option)
		{
			if (!option.template && !option.multiid)
			{
				removeValue(option.name);
				addValue(option.name);
			}
			else if (option.template)
			{
				// delete all multi-options
				for (var j=1; ; j++)
				{
					var optname = option.name.replace('1', j);
					if (!removeValue(optname))
					{
						break;
					}
				}
				// add all multi-options
				for (var j=1; ; j++)
				{
					var optname = option.name.replace('1', j);
					if (!addValue(optname))
					{
						break;
					}
				}
			}
		}

		for (var k=0; k < config.length; k++)
		{
			var conf = config[k];
			for (var i=0; i < conf.sections.length; i++)
			{
				var section = conf.sections[i];
				if (!section.hidden && selectedSections[section.id])
				{
					for (var m=0; m < section.options.length; m++)
					{
						restoreOption(section.options[m]);
					}
				}
			}
		}

		return values;
	}
}(jQuery));


/*** RESTORE SETTINGS DIALOG *******************************************************/

var RestoreSettingsDialog = (new function($)
{
	'use strict'

	// Controls
	var $RestoreSettingsDialog;
	var $SectionTable;

	// State
	var config;
	var restoreClick;

	this.init = function()
	{
		$RestoreSettingsDialog = $('#RestoreSettingsDialog');
		$('#RestoreSettingsDialog_Restore').click(restore);

		$SectionTable = $('#RestoreSettingsDialog_SectionTable');

		$SectionTable.fasttable(
			{
				pagerContainer: $('#RestoreSettingsDialog_SectionTable_pager'),
				rowSelect: UISettings.rowSelect,
				infoEmpty: I18n.translate('msg_no_sections_found'),
				pageSize: 1000
			});

		$RestoreSettingsDialog.on('hidden', function()
		{
			// cleanup
			$SectionTable.fasttable('update', []);
		});
	}

	this.showModal = function(_config, _restoreClick)
	{
		config = _config;
		restoreClick = _restoreClick;
		updateTable();
		$RestoreSettingsDialog.modal({backdrop: 'static'});
	}

	function updateTable()
	{
		var data = [];
		for (var k=0; k < config.length; k++)
		{
			var conf = config[k];
			for (var i=0; i < conf.sections.length; i++)
			{
				var section = conf.sections[i];
				if (!section.hidden)
				{
					var translated = I18n.defaultValue(('config_section_' + section.name.replace(/ /g, '_').replace(/-/g, '_')).toLowerCase(), section.name);
					var sectionName = translated;
					var spanHtml = '<span class="text-uppercase" data-section="' + section.id + '">' + sectionName + '</span>';
					var fields = ['<div class="check img-check"></div>', spanHtml];
					var item =
					{
						id: section.id,
						fields: fields,
					};
					data.push(item);
				}
			}
		}
		$SectionTable.fasttable('update', data);
	}

	function restore(e)
	{
		e.preventDefault();

		var checkedRows = $SectionTable.fasttable('checkedRows');
		var checkedCount = $SectionTable.fasttable('checkedCount');
		if (checkedCount === 0)
		{
			PopupNotification.show('#Notif_Config_RestoreSections');
			return;
		}

		checkedRows = $.extend({}, checkedRows); // clone
		$RestoreSettingsDialog.modal('hide');

		setTimeout(function() { restoreClick(checkedRows); }, 0);
	}

}(jQuery));


/*** UPDATE DIALOG *******************************************************/
var UpdateDialog = (new function($)
{
	'use strict'

	// Controls
	var $UpdateDialog;
	var $UpdateProgressDialog;
	var $UpdateProgressDialog_Log;
	var $UpdateDialog_Close;

	// State
	var VersionInfo;
	var PackageInfo;
	var UpdateInfo;
	var lastUpTimeSec;
	var installing = false;
	var logReceived = false;
	var foreground = false;

	this.init = function()
	{
		$UpdateDialog = $('#UpdateDialog');
		$('#UpdateDialog_InstallStable,#UpdateDialog_InstallTesting').click(install);
		$UpdateProgressDialog = $('#UpdateProgressDialog');
		$UpdateProgressDialog_Log = $('#UpdateProgressDialog_Log');
		$UpdateDialog_Close = $('#UpdateDialog_Close');

		$UpdateDialog.on('hidden', resumeRefresher);
		$UpdateProgressDialog.on('hidden', resumeRefresher);
	}

	function resumeRefresher()
	{
		if (!installing && foreground)
		{
			Refresher.resume();
		}
	}

	this.showModal = function()
	{
		foreground = true;
		this.performCheck();
		$UpdateDialog.modal({backdrop: 'static'});
	}

	this.checkUpdate = function()
	{
		// updates are off for now: RustyGet doesn't look for new releases
	}

	this.performCheck = function()
	{
		$('#UpdateDialog_Install').hide();
		$('#UpdateDialog_CheckProgress').show();
		$('#UpdateDialog_CheckFailed').hide();
		$('#UpdateDialog_Versions').hide();
		$('#UpdateDialog_UpdateAvail').hide();
		$('#UpdateDialog_DownloadAvail').hide();
		$('#UpdateDialog_UpdateNotAvail').hide();
		$('#UpdateDialog_InstalledInfo').show();
		$UpdateDialog_Close.text(foreground ? I18n.translate('btn_close') : I18n.translate('btn_remind_later'));
		$('#UpdateDialog_VerInstalled').text(Options.option('Version'));

		PackageInfo = {};
		VersionInfo = {};
		UpdateInfo = {};

		installing = false;
		if (foreground)
		{
			Refresher.pause();
		}

		// RustyGet has no public release feed: updates come only from the package's
		// own update source (package-info.json), never from another program's site.
		loadPackageInfo();
	}

	function parseJsonP(jsonp)
	{
		var p = jsonp.indexOf('{');
		var obj = JSON.parse(jsonp.substr(p, 10000));
		return obj;
	}

	function loadPackageInfo()
	{
		$.get('package-info.json', loadedPackageInfo, 'html').fail(loadedAll);
	}

	function loadedPackageInfo(data)
	{
		PackageInfo = parseJsonP(data);
		if (PackageInfo['update-info-link'])
		{
			RPC.call('readurl', [PackageInfo['update-info-link'], 'update info'], loadedUpdateInfo, loadedAll);
		}
		else if (PackageInfo['update-info-script'])
		{
			RPC.call('checkupdates', [], loadedUpdateInfo, loadedAll);
		}
		else
		{
			loadedAll();
		}
	}

	function loadedUpdateInfo(data)
	{
		UpdateInfo = parseJsonP(data);
		loadedAll();
	}

	function loadedAll()
	{
		var installedVersion = Options.option('Version');

		$('#UpdateDialog_CheckProgress').hide();
		$('#UpdateDialog_Versions').show();
		$('#UpdateDialog_InstalledInfo').show();

		$('#UpdateDialog_CurStable').text(VersionInfo['stable-version'] ? VersionInfo['stable-version'] : 'no data');
		$('#UpdateDialog_CurTesting').text(VersionInfo['testing-version'] ? VersionInfo['testing-version'] : 'no data');

		$('#UpdateDialog_CurNotesStable').attr('href', VersionInfo['stable-release-notes']);
		$('#UpdateDialog_CurNotesTesting').attr('href', VersionInfo['testing-release-notes']);
		$('#UpdateDialog_DownloadStable').attr('href', VersionInfo['stable-download']);
		$('#UpdateDialog_DownloadTesting').attr('href', VersionInfo['testing-download']);
		Util.show('#UpdateDialog_CurNotesStable', VersionInfo['stable-release-notes']);
		Util.show('#UpdateDialog_CurNotesTesting', VersionInfo['testing-release-notes']);


		$('#UpdateDialog_AvailStable').text(UpdateInfo['stable-version'] ? UpdateInfo['stable-version'] : 'not available');
		$('#UpdateDialog_AvailTesting').text(UpdateInfo['testing-version'] ? UpdateInfo['testing-version'] : 'not available');
		
		if (UpdateInfo['stable-version'] === VersionInfo['stable-version'] &&
			UpdateInfo['testing-version'] === VersionInfo['testing-version'])
		{
			$('#UpdateDialog_AvailStableBlock,#UpdateDialog_AvailTestingBlock').hide();
			$('#UpdateDialog_AvailRow .update-row-name').text('');
			$('#UpdateDialog_AvailRow td').css('border-style', 'none');
		}

		$('#UpdateDialog_DownloadRow td').css('border-style', 'none');
		
		$('#UpdateDialog_AvailNotesStable').attr('href', UpdateInfo['stable-package-info']);
		$('#UpdateDialog_AvailNotesTesting').attr('href', UpdateInfo['testing-package-info']);
		Util.show('#UpdateDialog_AvailNotesStableBlock', UpdateInfo['stable-package-info']);
		Util.show('#UpdateDialog_AvailNotesTestingBlock', UpdateInfo['testing-package-info']);


		var installedVer = installedVersion;

		// A plain string "<" sorts "26.10" below "26.3": compare the numeric parts.
		function versionLess(left, right)
		{
			if (!right)
			{
				return false;
			}
			return String(left).localeCompare(String(right), undefined, { numeric: true, sensitivity: 'base' }) < 0;
		}

		var canInstallStable = UpdateInfo['stable-version'] &&
			versionLess(installedVer, UpdateInfo['stable-version']);
		var canInstallTesting = UpdateInfo['testing-version'] &&
			versionLess(installedVer, UpdateInfo['testing-version']);
			  
			  
		Util.show('#UpdateDialog_InstallStable', canInstallStable);
		Util.show('#UpdateDialog_InstallTesting', canInstallTesting);

		var canDownloadStable =
			versionLess(installedVer, VersionInfo['stable-version']);
		var canDownloadTesting =
			versionLess(installedVer, VersionInfo['testing-version']);
			  
			  
		Util.show('#UpdateDialog_DownloadStable', canDownloadStable);
		Util.show('#UpdateDialog_DownloadTesting', canDownloadTesting);

		var hasUpdateSource = PackageInfo['update-info-link'] || PackageInfo['update-info-script'];
		var hasUpdateInfo = UpdateInfo['stable-version'] || UpdateInfo['testing-version'];
		var canUpdateStable = canInstallStable || canDownloadStable;
		var canUpdateTesting = canInstallTesting || canDownloadTesting;
		var canUpdate = (Options.option('UpdateCheck') === 'stable' && canUpdateStable) ||
			(Options.option('UpdateCheck') === 'testing' && canUpdateTesting);
		var canDownload = (Options.option('UpdateCheck') === 'stable' && canDownloadStable) ||
			(Options.option('UpdateCheck') === 'testing' && canDownloadTesting);
		Util.show('#UpdateDialog_UpdateAvail', canUpdate);
		Util.show('#UpdateDialog_UpdateNotAvail', Options.option('UpdateCheck') !== 'none' && !canUpdate && !canDownload);
		Util.show('#UpdateDialog_CheckFailed', hasUpdateSource && !hasUpdateInfo);
		Util.show('#UpdateDialog_DownloadRow,#UpdateDialog_DownloadAvail', canDownload && !canUpdate);
		$('#UpdateDialog_AvailRow').toggleClass('hide', !hasUpdateInfo);
		var shouldAutoPopupStable = Options.option('UpdateCheck') === 'stable' && (canInstallStable || canDownloadStable) && notificationAllowed('stable');
		var shouldAutoPopupTesting = Options.option('UpdateCheck') === 'testing' && (canInstallTesting || canDownloadTesting) && notificationAllowed('testing');
		if (shouldAutoPopupStable || shouldAutoPopupTesting)
		{
			$UpdateDialog.modal({backdrop: 'static'});
		}
	}

	function notificationAllowed(branch)
	{
		// We don't want to update all users on release day.
		// Parameter "update-rate" controls the spreading speed of the release.
		// It contains comma-separated list of percentages of users, which should get
		// notification about new release at a given day after release.

		var rateCurve = UpdateInfo[branch + '-update-rate'] ? UpdateInfo[branch + '-update-rate'] : VersionInfo[branch + '-update-rate'];
		if (!rateCurve)
		{
			return true;
		}

		var rates = rateCurve.split(',');
		var releaseDate = new Date(UpdateInfo[branch + '-date'] ? UpdateInfo[branch + '-date'] : VersionInfo[branch + '-date']);
		var daysSinceRelease = Math.floor((new Date() - releaseDate) / (1000*60*60*24));
		var coverage = rates[Math.min(daysSinceRelease, rates.length-1)];
		var dice = Math.floor(Math.random() * 100);

		return dice <= coverage;
	}

	function install(e)
	{
		e.preventDefault();
		var kind = $(this).attr('data-kind');
		var script = PackageInfo['install-script'];

		if (!script)
		{
			alert(I18n.translate('msg_package_info_error'));
			return;
		}

		RPC.call('status', [], function(status)
			{
				lastUpTimeSec = status.UpTimeSec;
				RPC.call('startupdate', [kind], updateStarted);
			});
	}

	function updateStarted(started)
	{
		if (!started)
		{
			PopupNotification.show('#Notif_StartUpdate_Failed');
			return;
		}

		installing = true;
		$UpdateDialog.fadeOut(250, function()
			{
				$UpdateProgressDialog_Log.text('');
				$UpdateProgressDialog.fadeIn(250, function()
					{
						$UpdateDialog.modal('hide');
						$UpdateProgressDialog.modal({backdrop: 'static'});
						updateLog();
					});
			});
	}

	function updateLog()
	{
		RPC.call('logupdate', [0, 100], function(data)
			{
				logReceived = logReceived || data.length > 0;
				if (logReceived && data.length === 0)
				{
					terminated();
				}
				else
				{
					updateLogTable(data);
					setTimeout(updateLog, 500);
				}
			}, terminated);
	}

	function terminated()
	{
		// rpc-failure: the program has been terminated. Waiting for new instance.
		setLogContentAndScroll($UpdateProgressDialog_Log.html() + '\n' + I18n.translate('msg_terminated_waiting'));
		setTimeout(checkStatus, 500);
	}

	function setLogContentAndScroll(html)
	{
		var scroll = $UpdateProgressDialog_Log.prop('scrollHeight') - $UpdateProgressDialog_Log.prop('scrollTop') === $UpdateProgressDialog_Log.prop('clientHeight');
		$UpdateProgressDialog_Log.html(html);
		if (scroll)
		{
			$UpdateProgressDialog_Log.scrollTop($UpdateProgressDialog_Log.prop('scrollHeight'));
		}
	}

	function updateLogTable(messages)
	{
		var html = '';
		for (var i=0; i < messages.length; i++)
		{
			var message = messages[i];
			var text = Util.textToHtml(message.Text);
			if (message.Kind === 'ERROR')
			{
				text = '<span class="update-log-error">' + text + '</span>';
			}
			html = html + text + '\n';
		}
		setLogContentAndScroll(html);
	}

	function checkStatus()
	{
		RPC.call('status', [], function(status)
			{
				// OK, checking if it is a restarted instance
				if (status.UpTimeSec >= lastUpTimeSec)
				{
					// the old instance is not restarted yet
					// waiting 0.5 sec. and retrying
					if ($('#UpdateProgressDialog').is(':visible'))
					{
						setTimeout(checkStatus, 500);
					}
				}
				else
				{
					// restarted successfully, refresh page
					setLogContentAndScroll($UpdateProgressDialog_Log.html() + '\n' + I18n.translate('msg_started_refreshing'));
					setTimeout(function()
						{
							document.location.reload(true);
						}, 1000);
				}
			},
			function()
			{
				// Failure, waiting 0.5 sec. and retrying
				if ($('#UpdateProgressDialog').is(':visible'))
				{
					setTimeout(checkStatus, 500);
				}
			});
	}

}(jQuery));

/*** FILE BROWSER DIALOG **********************************************************/

// Picks a file on the machine RustyGet runs on, listed by the daemon itself, so it
// shows what the daemon user can read (not what the browser's computer has).
var FileBrowserDialog = (new function($)
{
	'use strict';

	var $Dialog;
	var current = '';
	var selected = '';
	var onPick;
	var seq = 0;

	function t(key, text)
	{
		return I18n.defaultValue('file_browser_' + key, text);
	}

	function build()
	{
		$Dialog = $('<div class="modal modal-small modal-padded-small hide file-browser" id="FileBrowserDialog" role="dialog" aria-modal="true" aria-labelledby="FileBrowserDialog_Title">' +
			'<div class="modal-header"><a class="close" href="#" data-dismiss="modal"><i class="material-icon">close</i></a>' +
			'<h3 id="FileBrowserDialog_Title"></h3></div>' +
			'<div class="modal-body">' +
				'<div class="file-browser__bar"><button type="button" class="btn btn-default" id="FileBrowserDialog_Up"></button>' +
				'<input type="text" id="FileBrowserDialog_Path" class="file-browser__path" spellcheck="false"></div>' +
				'<p class="file-browser__note" id="FileBrowserDialog_Note"></p>' +
				'<ul class="file-browser__list" id="FileBrowserDialog_List" role="listbox"></ul>' +
			'</div>' +
			'<div class="modal-footer"><a href="#" class="btn btn-default" data-dismiss="modal"></a>' +
			'<a href="#" class="btn btn-primary" id="FileBrowserDialog_Choose"></a></div></div>');
		$Dialog.find('.close').attr('title', I18n.defaultValue('btn_close', 'Close'));
		$Dialog.find('#FileBrowserDialog_Up').text(t('up_short', 'Up')).attr('aria-label', t('up', 'Up one folder')).on('click', function()
		{
			load($Dialog.data('parent') || current);
		});
		$Dialog.find('[data-dismiss=modal].btn').text(I18n.defaultValue('btn_cancel', 'Cancel'));
		$Dialog.find('#FileBrowserDialog_Choose').text(t('choose', 'Choose')).on('click', function(e)
		{
			e.preventDefault();
			choose();
		});
		$Dialog.find('#FileBrowserDialog_Path').attr('aria-label', t('path', 'Folder or file')).on('keydown', function(e)
		{
			if (e.key === 'Enter')
			{
				e.preventDefault();
				load(this.value);
			}
		});
		$Dialog.find('#FileBrowserDialog_List').on('click', 'li', function()
		{
			var item = $(this);
			if (item.attr('aria-disabled') === 'true')
			{
				return;
			}
			if (item.data('dir'))
			{
				load(item.data('path'));
				return;
			}
			select(item);
		}).on('dblclick', 'li', function()
		{
			if (!$(this).data('dir') && $(this).attr('aria-disabled') !== 'true')
			{
				select($(this));
				choose();
			}
		}).on('keydown', 'li', function(e)
		{
			if (e.key === 'Enter' || e.key === ' ')
			{
				e.preventDefault();
				$(this).trigger('click');
			}
			else if (e.key === 'ArrowDown' || e.key === 'ArrowUp')
			{
				e.preventDefault();
				var next = e.key === 'ArrowDown' ? $(this).next('li') : $(this).prev('li');
				next.focus();
			}
		});
		$('body').append($Dialog);
	}

	function size(bytes)
	{
		return bytes < 1024 ? bytes + ' B' : bytes < 1048576 ? (bytes / 1024).toFixed(1) + ' KB' : Util.formatSizeMB(bytes / 1048576);
	}

	function join(dir, name)
	{
		return dir.replace(/\/+$/, '') + '/' + name;
	}

	function select(item)
	{
		$Dialog.find('li[aria-selected=true]').attr('aria-selected', 'false');
		item.attr('aria-selected', 'true');
		selected = item.data('path');
		$('#FileBrowserDialog_Path').val(selected);
		$('#FileBrowserDialog_Choose').toggleClass('disabled', false);
	}

	function choose()
	{
		// the path as shown: a selection the user then edited is not used
		var value = $.trim($('#FileBrowserDialog_Path').val());
		if (!value)
		{
			return;
		}
		$Dialog.modal('hide');
		if (onPick)
		{
			onPick(value);
		}
	}

	function note(text, error)
	{
		$('#FileBrowserDialog_Note').text(text || '').toggleClass('is-error', !!error).prop('hidden', !text);
	}

	function load(path)
	{
		var mine = ++seq;
		note(t('loading', 'Loading…'));
		RPC.call('browsefiles', [path || ''], function(result)
		{
			if (mine !== seq)
			{
				return;
			}
			render(result);
		}, function(message)
		{
			if (mine === seq)
			{
				note(String(message), true);
			}
		});
	}

	function render(result)
	{
		current = result.Path || current;
		$Dialog.data('parent', result.Parent || '');
		$('#FileBrowserDialog_Up').prop('disabled', !result.Parent || result.Parent === current);
		$('#FileBrowserDialog_Path').val(current);
		selected = '';
		var list = $('#FileBrowserDialog_List').empty();
		(result.Entries || []).forEach(function(entry)
		{
			var item = $('<li role="option" tabindex="0" aria-selected="false"><i class="material-icon" aria-hidden="true"></i><span class="file-browser__name"></span><span class="file-browser__size"></span></li>');
			item.find('.material-icon').text(entry.Dir ? 'folder' : 'description');
			item.find('.file-browser__name').text(entry.Name);
			item.find('.file-browser__size').text(entry.Dir ? '' : size(entry.Size));
			item.data('path', join(current, entry.Name)).data('dir', entry.Dir);
			if (!entry.Readable)
			{
				item.attr('aria-disabled', 'true').attr('title', t('no_access', 'RustyGet\'s user can\'t read this'));
			}
			list.append(item);
		});
		if (result.Error)
		{
			note(result.Error, true);
		}
		else if (!(result.Entries || []).length)
		{
			note(t('empty', 'This folder is empty.'));
		}
		else
		{
			note(result.Truncated ? t('truncated', 'Only the first 5000 entries are shown.') : '');
		}
	}

	this.showModal = function(title, start, pick)
	{
		if (!$Dialog)
		{
			build();
		}
		onPick = pick;
		$('#FileBrowserDialog_Title').text(title || t('title', 'Choose a file'));
		$Dialog.modal({ backdrop: 'static' });
		load(start);
	};
}(jQuery));
