/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2025 Denis <denis@nzbget.com>
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

#ifndef SECURITY_VALIDATOR_H
#define SECURITY_VALIDATOR_H

#include "SectionValidator.h"
#include "Validators.h"
#include "Options.h"

namespace SystemHealth::Security
{

class SecurityValidator final : public SectionValidator
{
public:
	explicit SecurityValidator(const Options& options) : m_options(options) {}
	std::string_view GetName() const override { return "Security"; }
	SectionReport Validate() const override;

private:
	const Options& m_options;
};

class ControlPasswordValidator final : public Validator
{
public:
	explicit ControlPasswordValidator(const Options& options) : m_options(options) {}
	std::string_view GetName() const override { return Options::CONTROLPASSWORD; }
	Status Validate() const override;

private:
	const Options& m_options;
};

class FormAuthValidator final : public Validator
{
public:
	explicit FormAuthValidator(const Options& options) : m_options(options) {}
	std::string_view GetName() const override { return Options::FORMAUTH; }
	Status Validate() const override;

private:
	const Options& m_options;
};

}  // namespace SystemHealth::Security

#endif
