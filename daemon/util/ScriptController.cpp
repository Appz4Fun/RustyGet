/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
#include "ScriptController.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "Options.h"
#include "nzbget_rs.h"

// System global variable holding environments variables
extern char** environ;
extern char* (*g_EnvironmentVariables)[];

ScriptController::RunningScripts ScriptController::m_runningScripts;
Mutex ScriptController::m_runningMutex;

const int FORK_ERROR_EXIT_CODE = 254;

#ifdef CHILD_WATCHDOG
/**
 * Sometimes the forked child process doesn't start properly and hangs
 * just during the starting. I didn't find any explanation about what
 * could cause that problem except of a general advice, that
 * "a forking in a multithread application is not recommended".
 *
 * Workaround:
 * 1) child process prints a line into stdout directly after the start;
 * 2) parent process waits for a line for 60 seconds. If it didn't receive it
 *    the child process is assumed to be hanging and will be killed. Another attempt
 *    will be made.
 */

class ChildWatchDog : public Thread
{
public:
	void SetProcessId(pid_t processId) { m_processId = processId; }
	void SetInfoName(const char* infoName) { m_infoName = infoName; }
protected:
	virtual void Run();
private:
	pid_t m_processId;
	CString m_infoName;
};

void ChildWatchDog::Run()
{
	static const int WAIT_SECONDS = 60;
	time_t start = Util::CurrentTime();
	while (!IsStopped() && (Util::CurrentTime() - start) < WAIT_SECONDS)
	{
		Util::Sleep(10);
	}

	if (!IsStopped())
	{
		info("Restarting hanging child process for %s", *m_infoName);
		kill(m_processId, SIGKILL);
	}
}
#endif


void EnvironmentStrings::Clear()
{
	m_strings.clear();
}

void EnvironmentStrings::InitFromCurrentProcess()
{
	if (!g_EnvironmentVariables)
		return;

	for (int i = 0; (*g_EnvironmentVariables)[i]; i++)
	{
		char* var = (*g_EnvironmentVariables)[i];
		// Ignore all env vars set by NZBGet.
		// This is to avoid the passing of env vars after program update (when NZBGet is
		// started from a script which was started by a previous instance of NZBGet).
		// Format: NZBXX_YYYY (XX are any two characters, YYYY are any number of any characters).
		if (!nzbget_rs_sus_is_nzbget_internal_env_var(var))
		{
			Append(var);
		}
	}
}

void EnvironmentStrings::Append(const char* envstr)
{
	m_strings.emplace_back(envstr);
}

void EnvironmentStrings::Append(CString&& envstr)
{
	m_strings.push_back(std::move(envstr));
}

/*
 * Returns environment block in format suitable for using with execve.
 */
std::vector<char*> EnvironmentStrings::GetStrings()
{
	std::vector<char*> strings;
	strings.reserve(m_strings.size() + 1);
	std::copy(m_strings.begin(), m_strings.end(), std::back_inserter(strings));
	strings.push_back(nullptr);
	return strings;
}


ScriptController::ScriptController()
{
	ResetEnv();

	Guard guard(m_runningMutex);
	m_runningScripts.push_back(this);
}

ScriptController::~ScriptController()
{
	UnregisterRunningScript();
}

void ScriptController::UnregisterRunningScript()
{
	Guard guard(m_runningMutex);
	m_runningScripts.erase(std::remove(m_runningScripts.begin(), m_runningScripts.end(), this), m_runningScripts.end());
}

void ScriptController::ResetEnv()
{
	m_environmentStrings.Clear();
	m_environmentStrings.InitFromCurrentProcess();
}

void ScriptController::SetEnvVar(const char* name, const char* value)
{
	NzbgetRsBuf envBuf = nzbget_rs_dqh_script_ctrl_format_env_assignment(name, value);
	m_environmentStrings.Append(CString(envBuf.data ? envBuf.data : "", (int)envBuf.len));
	nzbget_rs_free(envBuf);
}

void ScriptController::SetIntEnvVar(const char* name, int value)
{
	NzbgetRsBuf valBuf = nzbget_rs_dqh_conn_format_port_str(value);
	SetEnvVar(name, valBuf.data ? valBuf.data : "0");
	nzbget_rs_free(valBuf);
}

/**
 * If szStripPrefix is not nullptr, only options, whose names start with the prefix
 * are processed. The prefix is then stripped from the names.
 * If szStripPrefix is nullptr, all options are processed; without stripping.
 */
void ScriptController::PrepareEnvOptions(const char* stripPrefix)
{
	int prefixLen = stripPrefix ? strlen(stripPrefix) : 0;

	for (Options::OptEntry& optEntry : g_Options->GuardOptEntries())
	{
		const char* value = GetOptValue(optEntry.GetName(), optEntry.GetValue());
		if (nzbget_rs_ufs_script_ctrl_matches_env_prefix(optEntry.GetName(), stripPrefix, prefixLen) != 0)
		{
			SetEnvVarSpecial("NZBPO", optEntry.GetName() + prefixLen, value);
		}
		else if (!stripPrefix)
		{
			SetEnvVarSpecial("NZBOP", optEntry.GetName(), value);
		}
	}
}

void ScriptController::SetEnvVarSpecial(const char* prefix, const char* name, const char* value)
{
	NzbgetRsBuf varBuf = nzbget_rs_dqh_script_ctrl_format_prefixed_varname(prefix, name);
	BString<1024> varname;
	varname.Set(varBuf.data ? varBuf.data : "", (int)varBuf.len);
	nzbget_rs_free(varBuf);

	// Original name
	SetEnvVar(varname, value);

	NzbgetRsBuf normBuf = nzbget_rs_sus_normalize_special_env_var_name(varname);
	if (normBuf.data && nzbget_rs_dqh_bytes_equal(varname, normBuf.data) == 0)
	{
		SetEnvVar(normBuf.data, value);
	}
	nzbget_rs_free(normBuf);
}

void ScriptController::PrepareArgs()
{
	*m_cmdLine = '\0';
	m_cmdArgs.clear();

	if (m_args.size() == 1)
	{
		const auto found = Util::FindExecutorProgram(m_args[0].Str(), g_Options->GetShellOverride());
		int useFound = 0, prependArg = 0;
		nzbget_rs_tpr_script_resolve_single_arg_executor(
			found.has_value() ? 1 : 0,
			found ? found.value().c_str() : nullptr,
			m_args[0].Str(),
			&useFound, &prependArg);
		strncpy(m_cmdLine, useFound ? found.value().c_str() : (const char*)m_args[0], sizeof(m_cmdLine) - 1);
		// an interpreter goes before the script; a script run directly is
		// argv[0] already (m_args), not its own first argument too
		if (prependArg)
		{
			m_cmdArgs.emplace_back(found.value().c_str());
		}

		debug("CmdLine: %s", m_cmdLine);
	}
	else
	{
		strncpy(m_cmdLine, m_args[0], sizeof(m_cmdLine) - 1);
	}
}

int ScriptController::Execute()
{
	PrepareEnvOptions(nullptr);
	PrepareArgs();

	m_completed = false;
	m_processId = 0;
	if (m_terminated)
	{
		m_completed = true;
		return -1;
	}
	int exitCode = 0;

#ifdef CHILD_WATCHDOG
	bool childConfirmed = false;
	while (!childConfirmed && !m_terminated)
	{
#endif

	int pipein = -1, pipeout = -1;
	StartProcess(&pipein, &pipeout);
	if (pipein == -1)
	{
		m_completed = true;
		return -1;
	}

	// open the read end
	m_readpipe = fdopen(pipein, "r");
	if (!m_readpipe)
	{
		PrintMessage(Message::mkError, "Could not open read pipe to %s", *m_infoName);
		close(pipein);
		if (pipeout != -1)
		{
			close(pipeout);
		}
		m_completed = true;
		return -1;
	}

	m_writepipe = 0;
	if (m_needWrite)
	{
		// open the write end
		m_writepipe = fdopen(pipeout, "w");
		if (!m_writepipe)
		{
			PrintMessage(Message::mkError, "Could not open write pipe to %s", *m_infoName);
			fclose(m_readpipe.exchange(nullptr));
			close(pipeout);
			m_completed = true;
			return -1;
		}
	}

#ifdef CHILD_WATCHDOG
	debug("Creating child watchdog");
	ChildWatchDog watchDog;
	watchDog.SetAutoDestroy(false);
	watchDog.SetProcessId(m_processId);
	watchDog.SetInfoName(m_infoName);
	watchDog.Start();
#endif

	CharBuffer buf(1024 * 10);

	debug("Entering pipe-loop");
	bool firstLine = true;
	bool startError = false;
	while (!m_terminated && !m_detached)
	{
		FILE* readpipe = m_readpipe;
		if (!readpipe || feof(readpipe))
		{
			break;
		}
		if (ReadLine(buf, buf.Size(), readpipe) && m_readpipe)
		{
#ifdef CHILD_WATCHDOG
			if (!childConfirmed)
			{
				childConfirmed = true;
				watchDog.Stop();
				debug("Child confirmed");
				continue;
			}
#endif
			if (nzbget_rs_ufs_script_ctrl_is_fork_start_error_line(firstLine ? 1 : 0, buf) != 0)
			{
				startError = true;
			}
			ProcessOutput(buf);
			firstLine = false;
		}
	}
	debug("Exited pipe-loop");

#ifdef CHILD_WATCHDOG
	debug("Destroying WatchDog");
	if (!childConfirmed)
	{
		watchDog.Stop();
	}
	while (watchDog.IsRunning())
	{
		Util::Sleep(5);
	}
#endif

	if (FILE* readpipe = m_readpipe.exchange(nullptr))
	{
		fclose(readpipe);
	}

	if (m_writepipe)
	{
		fclose(m_writepipe);
	}

	if (m_terminated && m_infoName)
	{
		warn("Interrupted %s", *m_infoName);
	}

	exitCode = 0;

	if (!m_detached)
	{
		exitCode = WaitProcess();
		exitCode = nzbget_rs_ufs_script_ctrl_normalize_fork_exit_code(
			exitCode, FORK_ERROR_EXIT_CODE, startError ? 1 : 0);
	}

#ifdef CHILD_WATCHDOG
	}	// while (!bChildConfirmed && !m_bTerminated)
#endif

	debug("Exit code %i", exitCode);
	m_completed = true;
	m_processId = 0;
	return exitCode;
}

/*
* Returns file descriptor of the read-pipe or -1 on error.
*/
void ScriptController::StartProcess(int* pipein, int* pipeout)
{
	CString workingDir = m_workingDir;
	if (workingDir.Empty())
	{
		workingDir = FileSystem::GetCurrentDirectory();
	}

	const char* script = m_args[0];

	int pin[] = {-1, -1};
	int pout[] = {-1, -1};

	// create the pipes, close-on-exec: a script another thread starts at the
	// same time would otherwise inherit our write end and keep it open for as
	// long as it runs - we'd see no end of output until that script ended too.
	// dup2() onto the child's stdin/stdout/stderr clears the flag there.
	auto makePipe = [](int fds[2])
	{
#ifdef __linux__
		return pipe2(fds, O_CLOEXEC);
#else
		if (pipe(fds))
		{
			return -1;
		}
		fcntl(fds[0], F_SETFD, FD_CLOEXEC);
		fcntl(fds[1], F_SETFD, FD_CLOEXEC);
		return 0;
#endif
	};
	if (makePipe(pin))
	{
		PrintMessage(Message::mkError, "Could not open read pipe: errno %i", errno);
		return;
	}
	if (m_needWrite && makePipe(pout))
	{
		PrintMessage(Message::mkError, "Could not open write pipe: errno %i", errno);
		close(pin[0]);
		close(pin[1]);
		return;
	}

	std::vector<char*> environmentStrings = m_environmentStrings.GetStrings();
	char** envdata = environmentStrings.data();

	CheckEnvSize(environmentStrings);

	std::copy(std::begin(m_args), std::end(m_args), std::back_inserter(m_cmdArgs));
	m_cmdArgs.emplace_back(nullptr);
	char* const* argdata = (char* const*)m_cmdArgs.data();

#ifdef DEBUG
	debug("Starting  process: %s", script);
	for (const char* arg : m_args)
	{
		debug("arg: %s", arg);
	}
#endif

	debug("forking");
	pid_t pid = fork();

	if (pid == -1)
	{
		PrintMessage(Message::mkError, "Could not start %s: errno %i", *m_infoName, errno);
		close(pin[0]);
		close(pin[1]);
		if (m_needWrite)
		{
			close(pout[0]);
			close(pout[1]);
		}
		return;
	}
	else if (pid == 0)
	{
		// here goes the second instance

		// only certain functions may be used here or the program may hang.
		// for a list of functions see chapter "async-signal-safe functions" in
		// http://man7.org/linux/man-pages/man7/signal.7.html

		// create new process group (see Terminate() where it is used)
		setsid();

		// make the pipeout to be the same as stdout and stderr
		dup2(pin[1], 1);
		dup2(pin[1], 2);
		close(pin[0]);
		close(pin[1]);

		if (m_needWrite)
		{
			// make the pipein to be the same as stdin
			dup2(pout[0], 0);
			close(pout[0]);
			close(pout[1]);
		}

#ifdef CHILD_WATCHDOG
		write(1, "\n", 1);
		fsync(1);
#endif

		chdir(workingDir);
		environ = envdata;

		execvp(m_cmdLine, argdata);

		if (errno == EACCES)
		{
			write(1, "[WARNING] Fixing permissions for", 32);
			write(1, script, strlen(script));
			write(1, "\n", 1);
			fsync(1);
			FileSystem::FixExecPermission(script);
			execvp(m_cmdLine, argdata);
		}

		// NOTE: the text "[ERROR] Could not start " is checked later,
		// by changing adjust the dependent code below.
		write(1, "[ERROR] Could not start ", 24);
		write(1, script, strlen(script));
		write(1, ": ", 2);
		char* errtext = strerror(errno);
		write(1, errtext, strlen(errtext));
		write(1, "\n", 1);
		fsync(1);
		_exit(FORK_ERROR_EXIT_CODE);
	}

	// continue the first instance
	debug("forked");
	debug("Child Process-ID: %i", (int)pid);

	m_processId = pid;

	// close unused pipe ends; the caller gets the others only now: on a
	// failed fork they were closed already
	close(pin[1]);
	if (m_needWrite)
	{
		close(pout[0]);
	}
	*pipein = pin[0];
	*pipeout = pout[1];
}

int ScriptController::WaitProcess()
{
	int status = 0;
	pid_t res;
	do
	{
		res = waitpid(m_processId, &status, 0);
	} while (res == -1 && errno == EINTR);
	if (res == -1)
	{
		return -1;
	}
	if (WIFEXITED(status))
	{
		int exitCode = WEXITSTATUS(status);
		return exitCode;
	}
	if (WIFSIGNALED(status))
	{
		// not a success: a crashed or killed program used to read as exit
		// code 0. The shell's convention, 128 + signal.
		if (!m_terminated)
		{
			PrintMessage(Message::mkWarning, "%s was killed by signal %i", *m_infoName, WTERMSIG(status));
		}
		return nzbget_rs_ufs_script_ctrl_calc_signal_exit_code(WTERMSIG(status));
	}
	return -1;
}

bool ScriptController::HasActiveProcess()
{
	return m_processId.load() > 0 && !m_completed;
}

void ScriptController::Terminate()
{
	debug("Stopping %s", *m_infoName);
	m_terminated = true;

	pid_t childPid = m_processId.load();
	pid_t killId = (pid_t)nzbget_rs_ufs_script_ctrl_select_kill_pid(
		(int)childPid, childPid > 0 ? (int)getpgid(childPid) : 0);
	int killRes = childPid > 0 ? kill(killId, SIGKILL) : 0;
	bool ok = nzbget_rs_dqh_script_ctrl_is_terminate_ok(
		(int)childPid, killRes, m_completed ? 1 : 0) != 0;

	if (ok)
	{
		debug("Terminated %s", *m_infoName);
	}
	else
	{
		error("Could not terminate %s", *m_infoName);
	}

	debug("Stopped %s", *m_infoName);
}

void ScriptController::TerminateAll()
{
	Guard guard(m_runningMutex);
	for (ScriptController* script : m_runningScripts)
	{
		if (script->HasActiveProcess() && !script->m_detached)
		{
			// send break signal and wait up to 5 seconds for graceful termination
			if (script->Break())
			{
				time_t curtime = Util::CurrentTime();
				while (!script->m_completed && std::abs(curtime - Util::CurrentTime()) <= 10)
				{
					Util::Sleep(100);
				}
			}
			script->Terminate();
		}
	}
}

bool ScriptController::Break()
{
	debug("Sending break signal to %s", *m_infoName);

	pid_t pid = m_processId.load();
	bool ok = nzbget_rs_dqh_script_ctrl_can_send_break((int)pid, m_completed ? 1 : 0) != 0 &&
		kill(pid, SIGINT) == 0;

	if (ok)
	{
		debug("Sent break signal to %s", *m_infoName);
	}
	else
	{
		warn("Could not send break signal to %s", *m_infoName);
	}

	return ok;
}

void ScriptController::Detach()
{
	debug("Detaching %s", *m_infoName);
	m_detached = true;
	if (FILE* readpipe = m_readpipe.exchange(nullptr))
	{
		fclose(readpipe);
	}
}

void ScriptController::Resume()
{
	m_terminated = false;
	m_detached = false;
	m_processId = 0;
}

bool ScriptController::ReadLine(char* buf, int bufSize, FILE* stream)
{
	return fgets(buf, bufSize, stream);
}

void ScriptController::ProcessOutput(char* text)
{
	debug("Processing output received from script");

	int msgKind = 0;
	size_t msgOffset = 0;
	if (!nzbget_rs_sus_classify_script_output_line(text, &msgKind, &msgOffset))
	{
		return;
	}
	Message::EKind kind = Message::mkInfo;
	switch (msgKind)
	{
		case 1: kind = Message::mkWarning; break;
		case 2: kind = Message::mkError; break;
		case 3: kind = Message::mkDetail; break;
		case 4: kind = Message::mkDebug; break;
		default: kind = Message::mkInfo; break;
	}
	PrintMessage(kind, "%s", text + msgOffset);

	debug("Processing output received from script - completed");
}

void ScriptController::AddMessage(Message::EKind kind, const char* text)
{
	switch (kind)
	{
		case Message::mkDetail:
			detail("%s", text);
			break;

		case Message::mkInfo:
			info("%s", text);
			break;

		case Message::mkWarning:
			warn("%s", text);
			break;

		case Message::mkError:
			error("%s", text);
			break;

		case Message::mkDebug:
			debug("%s", text);
			break;
	}
}

void ScriptController::PrintMessage(Message::EKind kind, const char* format, ...)
{
	// not cut to a fixed size: subclasses parse commands ("[NZB] ...") from it
	CString tmp2;

	va_list ap;
	va_start(ap, format);
	tmp2.FormatV(format, ap);
	va_end(ap);

	if (m_logPrefix)
	{
		NzbgetRsBuf prefBuf = nzbget_rs_dqh_script_format_prefixed_message(m_logPrefix, *tmp2);
		AddMessage(kind, prefBuf.data ? prefBuf.data : "");
		nzbget_rs_free(prefBuf);
	}
	else
	{
		AddMessage(kind, tmp2);
	}
}

void ScriptController::Write(const char* str)
{
	fwrite(str, 1, strlen(str), m_writepipe);
	fflush(m_writepipe);
}

void ScriptController::CheckEnvSize(const std::vector<char*>& envs)
{
	if (ARGS_LIMIT_SIZE <= 0) return;

	size_t limit = static_cast<size_t>(ARGS_LIMIT_SIZE);
	size_t exceededTotal = 0;
	if (nzbget_rs_sus_check_env_size_limit(
		reinterpret_cast<const char* const*>(envs.data()),
		envs.size(),
		sizeof(void*),
		limit,
		&exceededTotal))
	{
		PrintMessage(Message::EKind::mkWarning,
					 "Total environment size (%zu bytes) exceeds OS limit (%zu bytes): %s",
					 exceededTotal, limit, "This may cause process creation to fail");
	}
}
