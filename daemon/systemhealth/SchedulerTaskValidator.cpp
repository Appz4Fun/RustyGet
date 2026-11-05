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

#include "nzbget.h"

#include "SchedulerTaskValidator.h"
#include "nzbget_rs.h"

namespace SystemHealth::Scheduler
{
SchedulerTaskValidator::SchedulerTaskValidator(const ::Scheduler::Task& task)
	: m_task(task), m_name("Task" + std::to_string(m_task.GetId()))
{
}

SectionReport SchedulerTaskValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	nzbget_rs_health_validate_scheduler_task_section(
		m_task.GetHours(),
		m_task.GetMinutes(),
		m_task.GetWeeDaysBits(),
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}

}  // namespace SystemHealth::Scheduler
