/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 */

#include "nzbget.h"

#include "SchedulerTasksValidator.h"
#include "SchedulerTaskValidator.h"
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

namespace SystemHealth::Scheduler
{
SchedulerTasksValidator::SchedulerTasksValidator(const ::Scheduler::TaskList& tasks)
	: SectionGroupValidator(MakeTaskValidators(tasks)), m_tasks(tasks)
{
}

std::vector<std::unique_ptr<SectionValidator>> SchedulerTasksValidator::MakeTaskValidators(
	const ::Scheduler::TaskList& tasks) const
{
	std::vector<std::unique_ptr<SectionValidator>> validators;
#ifdef NZBGET_USE_RUST
	validators.reserve(nzbget_rs_awc_health_scheduler_tasks_reserve_count(tasks.size()));
#else
	validators.reserve(tasks.size());
#endif
	for (const auto& task : tasks)
	{
		validators.push_back(std::make_unique<SchedulerTaskValidator>(*task));
	}
	return validators;
}

}  // namespace SystemHealth::SchedulerTask
