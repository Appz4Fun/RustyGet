/*
 *  This file is part of RustyGet.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

#ifndef TEST_FRAMEWORK_H
#define TEST_FRAMEWORK_H

#include <cstdio>
#include <cstring>
#include <exception>
#include <functional>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace TestFramework
{
	struct RequireFailure : std::runtime_error
	{
		using std::runtime_error::runtime_error;
	};

	struct TestCase
	{
		std::string suite;
		std::string name;
		std::function<void()> fn;
	};

	inline std::vector<TestCase>& Registry()
	{
		static std::vector<TestCase> cases;
		return cases;
	}

	inline std::vector<std::function<std::shared_ptr<void>()>>& Fixtures()
	{
		static std::vector<std::function<std::shared_ptr<void>()>> fixtures;
		return fixtures;
	}

	inline int& FailCount()
	{
		static int count = 0;
		return count;
	}

	inline void RecordFailure(const char* file, int line, const std::string& msg, bool fatal)
	{
		++FailCount();
		std::cerr << file << ":" << line << ": error: " << msg << "\n";
		if (fatal)
		{
			throw RequireFailure(msg);
		}
	}

	template <typename A, typename B>
	bool AreEqual(const A& a, const B& b)
	{
		if constexpr (std::is_integral_v<A> && std::is_integral_v<B> && !std::is_same_v<A, bool> && !std::is_same_v<B, bool>)
		{
			using Common = std::common_type_t<A, B>;
			return static_cast<Common>(a) == static_cast<Common>(b);
		}
		else
		{
			return a == b;
		}
	}

	template <typename A, typename B>
	void CheckEqual(const A& a, const B& b, const char* exprA, const char* exprB, const char* file, int line, bool fatal)
	{
		if (!AreEqual(a, b))
		{
			std::ostringstream oss;
			oss << "check " << exprA << " == " << exprB << " has failed";
			RecordFailure(file, line, oss.str(), fatal);
		}
	}

	template <typename A, typename B>
	void CheckNe(const A& a, const B& b, const char* exprA, const char* exprB, const char* file, int line, bool fatal)
	{
		if (AreEqual(a, b))
		{
			std::ostringstream oss;
			oss << "check " << exprA << " != " << exprB << " has failed";
			RecordFailure(file, line, oss.str(), fatal);
		}
	}

	inline int RunAll(int argc, char* argv[])
	{
		std::string filter;
		for (int i = 1; i < argc; ++i)
		{
			std::string_view arg(argv[i]);
			if (arg.starts_with("--run_test="))
			{
				filter = std::string(arg.substr(strlen("--run_test=")));
			}
		}

		std::vector<std::shared_ptr<void>> activeFixtures;
		for (auto& f : Fixtures())
		{
			activeFixtures.push_back(f());
		}

		int executed = 0;
		for (const auto& tc : Registry())
		{
			std::string fullName = tc.suite.empty() ? tc.name : (tc.suite + "/" + tc.name);
			if (!filter.empty() && filter != tc.suite && filter != tc.name && filter != fullName)
			{
				continue;
			}
			++executed;
			try
			{
				tc.fn();
			}
			catch (const RequireFailure&)
			{
			}
			catch (const std::exception& e)
			{
				++FailCount();
				std::cerr << "Unhandled exception in " << fullName << ": " << e.what() << "\n";
			}
			catch (...)
			{
				++FailCount();
				std::cerr << "Unhandled unknown exception in " << fullName << "\n";
			}
		}

		activeFixtures.clear();

		if (FailCount() == 0)
		{
			std::cout << "*** No errors detected (" << executed << " test cases)\n";
			return 0;
		}
		else
		{
			std::cerr << "*** " << FailCount() << " failure(s) detected\n";
			return 1;
		}
	}
}

#define BOOST_AUTO_TEST_SUITE(SuiteName) \
	namespace Suite_##SuiteName { \
		static constexpr const char* CURRENT_SUITE_NAME = #SuiteName;

#define BOOST_AUTO_TEST_SUITE_END() \
	}

#define BOOST_AUTO_TEST_CASE(TestName) \
	static void TestName##_Impl(); \
	namespace { \
		struct TestName##_Registrar { \
			TestName##_Registrar() { \
				::TestFramework::Registry().push_back({CURRENT_SUITE_NAME, #TestName, &TestName##_Impl}); \
			} \
		} g_##TestName##_registrar; \
	} \
	static void TestName##_Impl()

#define BOOST_GLOBAL_FIXTURE(FixtureType) \
	namespace { \
		struct FixtureType##_Registrar { \
			FixtureType##_Registrar() { \
				::TestFramework::Fixtures().push_back([]() -> std::shared_ptr<void> { \
					return std::make_shared<FixtureType>(); \
				}); \
			} \
		} g_##FixtureType##_registrar; \
	}

#define BOOST_CHECK(cond) \
	do { \
		if (!(cond)) { \
			::TestFramework::RecordFailure(__FILE__, __LINE__, std::string("check ") + #cond + " has failed", false); \
		} \
	} while (0)

#define BOOST_REQUIRE(cond) \
	do { \
		if (!(cond)) { \
			::TestFramework::RecordFailure(__FILE__, __LINE__, std::string("require ") + #cond + " has failed", true); \
		} \
	} while (0)

#define BOOST_CHECK_MESSAGE(cond, msg) \
	do { \
		if (!(cond)) { \
			std::ostringstream _tf_oss; \
			_tf_oss << msg; \
			::TestFramework::RecordFailure(__FILE__, __LINE__, _tf_oss.str(), false); \
		} \
	} while (0)

#define BOOST_REQUIRE_MESSAGE(cond, msg) \
	do { \
		if (!(cond)) { \
			std::ostringstream _tf_oss; \
			_tf_oss << msg; \
			::TestFramework::RecordFailure(__FILE__, __LINE__, _tf_oss.str(), true); \
		} \
	} while (0)

#define BOOST_CHECK_EQUAL(a, b) \
	::TestFramework::CheckEqual((a), (b), #a, #b, __FILE__, __LINE__, false)

#define BOOST_REQUIRE_EQUAL(a, b) \
	::TestFramework::CheckEqual((a), (b), #a, #b, __FILE__, __LINE__, true)

#define BOOST_CHECK_NE(a, b) \
	::TestFramework::CheckNe((a), (b), #a, #b, __FILE__, __LINE__, false)

#endif
