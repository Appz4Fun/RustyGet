/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2025-2026 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"

#include "Status.h"
#include "ExtensionScriptsValidator.h"
#include "Validators.h"
#include "Options.h"
#include "Util.h"
#include "nzbget_rs.h"

namespace SystemHealth::ExtensionScripts
{
SectionReport ExtensionScriptsValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	std::string_view extensions = m_options.GetExtensions();
	std::string_view shellOverride = m_options.GetShellOverride();

	std::vector<ExtensionManager::ExtensionPtr> extHolders;
	std::vector<const char*> extNamePtrs;
	std::vector<size_t> extNameLens;
	std::vector<const char*> extEntryPtrs;
	std::vector<size_t> extEntryLens;
	m_extensionManager.ForEach([&](const ExtensionManager::ExtensionPtr ext)
	{
		extHolders.push_back(ext);
		std::string_view name = ext->GetName();
		std::string_view entry = ext->GetEntry();
		extNamePtrs.push_back(name.data());
		extNameLens.push_back(name.size());
		extEntryPtrs.push_back(entry.data());
		extEntryLens.push_back(entry.size());
	});

	nzbget_rs_health_validate_extension_scripts_section(
		extensions.data(), extensions.size(),
		extNamePtrs.data(), extNameLens.data(),
		extEntryPtrs.data(), extEntryLens.data(),
		extNamePtrs.size(),
		shellOverride.data(), shellOverride.size(),
		m_options.GetEventInterval(),
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}

Status ExtensionListValidator::Validate() const
{
	std::string_view extensions = m_options.GetExtensions();
	std::vector<ExtensionManager::ExtensionPtr> extHolders;
	std::vector<const char*> extNamePtrs;
	std::vector<size_t> extNameLens;
	std::vector<const char*> extEntryPtrs;
	std::vector<size_t> extEntryLens;
	m_extensionManager.ForEach([&](const ExtensionManager::ExtensionPtr ext)
	{
		extHolders.push_back(ext);
		std::string_view name = ext->GetName();
		std::string_view entry = ext->GetEntry();
		extNamePtrs.push_back(name.data());
		extNameLens.push_back(name.size());
		extEntryPtrs.push_back(entry.data());
		extEntryLens.push_back(entry.size());
	});

	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_extension_list(
		extensions.data(), extensions.size(),
		extNamePtrs.data(), extNameLens.data(),
		extEntryPtrs.data(), extEntryLens.data(),
		extNamePtrs.size(),
		&msg);
	return Status::FromRust(sev, msg);
}
}  // namespace SystemHealth::ExtensionScripts
