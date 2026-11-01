## Functional testing

During functional testing the program is tested as a whole. This is much like if the testing were made by a human. Although the user interface parts are not tested the core of the program is tested and how the different program components work together. When functional tests succeed that means that the tested scenarios will most likely work properly when the program is run by real users. The quality of total testing obviously depends on the amount and quality of individual tests.

**Note:** In addition to functional tests there are **unit tests**, which are not covered by this document.

## Infrastructure

The functional testing in NZBGet is controlled by scripts, which do things like:

- automatically start nzbget binary with a specific configuration;
- communicate with nzbget using nzbget API to control the program;
- create test data files and nzb-files referring to them;
- automatically start a special NNTP server, which serves test data files;
- tell nzbget to download test nzb-files from the test NNTP server;
- control the status of nzbget during download;
- check if the test downloads go as expected (success, failure);
- collect the results of all tests and represent the result summary.

## NServ - built-in NNTP server

  
NZBGet downloads from Usenet servers. However it would be unreliable and slow to use global Usenet servers for functional testing. Instead we need to do all the testing locally using our set of test files. For this purpose a simple NNTP server called **NServ** was developed.

Technically NServ is integrated into NZBGet as a sub-module. NServ is automatically controlled (started, stopped) by functional testing scripts but it can also be used standalone for other purposes such as debugging NZBGet or for speed tests of NZBGet or even other download clients. For more info see [NServ NNTP server](NSERV.md).

## Testing framework
  
Functional tests for NZBGet are written in python language (Python 3). We are using testing framework [pytest](https://pytest.org) to organize test execution and collect test results. Therefore in order to run functional tests pytest [must be installed](https://docs.pytest.org/en/latest/getting-started.html) on your system.
  
## Running tests

Functional tests and supporting modules are stored in directory `tests/functional`. To run tests open command prompt in this directory and execute `pytest` as explained below.

### Configuring tests
  
Tests can be run without extra configuration in which case the test files are stored in `tests/testdata/nserv.temp` and temporary files created by NZBGet during testing are stored in `tests/testdata/nzbget.temp`. Both of these directories take several gigabytes of disk space and can be reconfigured to use other locations.

Create text file `tests/functional/pytest.ini` with the following content:

```bash
  [pytest]
  nzbget_bin=/path/to/nzbget
  sevenzip_bin=/path/to/7za
  nserv_datadir=/path/to/nserv.temp
  nzbget_maindir=/path/to/nzbget.temp
```
  
All settings in the ini-file are optional.

### Preparing test files

When executing the tests for the first time the test scripts prepare test files and put them into directory `tests/testdata/nserv.temp`. These files take several gigabytes and may need several minutes to generate. When starting tests for the first time it’s recommended to add parameter `-s` to see additional progress logging during preparation stage:

```bash
  pytest -v -s
```

### Executing tests

To run all tests use simple command:

```bash
  pytest -v
```

To run tests from one test script only pass the specific file name:

```bash
  pytest -v parcheck/parcheck_force_test.py
```

To execute only one specific test pass the file name and test function name:

```bash
  pytest -v parcheck/parcheck_auto_test.py::test_parchecker_repair
```
  
For more filter possibilities please see [pytest](https://docs.pytest.org/en/latest/usage.html) documentation.

### Test failures
  
Directory used by NZBGet during testing (`tests/testdata/nzbget.temp` by default) is automatically deleted if all tests succeed. If a test fails the directory is kept in order to preserve log-files and queue-files for failure analysis. However during testing NZBGet is started and stopped multiple times. If the failed test wasn’t the last test the preserved NZBGet directory may not contain data of the failed test. In such case it’s better to rerun the specific failed test as the only test (using specific command line).

After test completion NZBGet is terminated. To analyze a test failure it can be useful to have NZBGet running, for example to inspect the state in web-interface. Pass extra parameter `--hold` to achieve this:

```bash
  pytest -v parcheck/parcheck_auto_test.py::test_parchecker_repair --hold
```

## DupeArticleFallback harness

`tests/functional/dupefallback/harness.py` drives a scratch daemon and nzbget's
own test news server (`nzbget --nserv`) through several hundred offline
scenarios of the duplicate features: article borrowing, stream and live repair,
whole-file recreation, par-repair hand-offs, `HealthCheck=dupe` failover, the
duplicate search against a fake Newznab indexer, and the queue and API edges
around them. It runs on Linux and macOS and needs Python 3 with
`cryptography`, plus `7z` and `unrar` on PATH for the archive scenarios; a
scenario whose tool or module is missing is reported as `SKIP` and the run
exits 77, never green.

```sh
tests/functional/dupefallback/run-local.sh /absolute/build/nzbget            # every scenario
tests/functional/dupefallback/run-local.sh /absolute/build/nzbget stream,livegate
tests/functional/dupefallback/run-linux-docker.sh /absolute/linux-x86_64/nzbget
```

A failed scenario keeps its work directory (the daemon's `nzbget.log`, its
configuration and queue) and prints the path.

### The mode matrix

`DupeArticleFallback` is ordered `no < article < stream < live` and the daemon
gates its features with `>=`, so a scenario written for a lower mode must pass
under a higher one. `--fallback-mode live` (or `stream`) runs every scenario
written for a lower mode with its `DupeArticleFallback=` option raised to that
mode; scenarios written for `no`, scenarios already at or above the requested
mode, and the few in `FALLBACK_MODE_BOUND` (they assert the sequence of events
of their own mode) are left out.

```sh
python3 tests/functional/dupefallback/harness.py --nzbget /absolute/build/nzbget --fallback-mode live
python3 tests/functional/dupefallback/harness.py --nzbget /absolute/build/nzbget --fallback-mode stream
```

### A real indexer

`dupesearchrealindexer` runs the duplicate search against a real Newznab
indexer (NZBHydra2) instead of the fake one. It needs `NZBHYDRA2_URL` (the
api endpoint, for example `http://hydra.local:5076/api`; the base URL from
the browser's address bar works too, `/api` is appended) and
`NZBHYDRA2_APIKEY`, from the environment or from a `.env` file in the
repository root (ignored by git); without them it is a `SKIP`. It asks the
indexer for its newest scene-named TV posting with the fewest other listings,
appends that title as a pick whose articles the fake news server lacks, and
checks that the daemon finds the release again with its own queries, fetches
the nzb-files of its listings through the indexer and judges them, with no
indexer error, no unreadable answer and no api key in the log. It is a dry
run and queues nothing, but every nzb-file fetched counts as a grab at the
indexers behind NZBHydra2: run it on purpose, not in every loop. The api key
lands in the scratch daemon's configuration, which a failed run keeps; do not
publish such a directory.

## TLS cipher optimizer fixture

`fixture.py` starts a candidate NZBGet binary with a new configuration, queue,
folders and four synthetic TLS/NNTP servers on loopback. It never reads an
existing NZBGet configuration. All servers are disabled for ordinary downloads;
the optimizer deliberately includes disabled encrypted backup servers.

Run this only in the runtime stage assigned under the repository evidence and
worker procedures. Use absolute paths for the candidate executable, current
WebUI, `nzbget.conf` template, assigned OpenSSL executable and a **new** output
directory. The output directory must not already exist.

```sh
python3 -B tests/functional/tlscipher/fixture.py \
  --binary /absolute/candidate/nzbget \
  --webui /absolute/source/webui \
  --template /absolute/source/nzbget.conf \
  --openssl /absolute/assigned/openssl \
  --root /absolute/new/exclusive-output
```

The automatic exercise has three predeclared jobs:

1. Measure local authenticated decryption and probe two providers with different
   AES-GCM support sets, one with rejected authentication, and one with an
   untrusted certificate. Check the selected cipher against the actual measured
   ranking, successful negotiated suite/version, per-server outcomes, common-set
   unknown state, cipher-only file changes and retained file mode. A fifth,
   plaintext server must remain untouched and absent from the probe results.
2. Cancel a new job and verify that configuration bytes remain unchanged.
3. Reset one fixture cipher before a new snapshot, edit an unrelated config
   comment after that snapshot, and verify that the commit refuses the conflict
   while retaining every byte of the external edit.

Each job's full result and observed states are retained. Provider events contain
negotiated suites and command verbs, never account values. No ARTICLE, BODY or
GROUP requests are allowed. The fixture closes and reaps its daemon and joins
its provider threads on exit. Generator and daemon command, process and exit
records are retained, including failures. The surrounding assigned controller
owns overall runtime, process, output and resource limits.

For an isolated manual test, add `--serve --port 17689` under the assigned
interactive runtime stage. Open the printed loopback URL, choose **Settings →
News Servers → Optimize TLS ciphers**, and inspect speed estimates, per-server
compatibility details and saved results. This local fixture uses blank WebUI
control credentials and binds only to loopback. Ctrl-C closes its owned daemon
and providers. It does not reload NZBGet after saving ciphers.

OpenSSL is used to generate synthetic fixture certificates. The product measures
the daemon's linked TLS library through native callbacks and has no external
OpenSSL executable dependency. These checks verify behavior; synthetic throughput
results are not native language, download-speed or release qualification.

Two additional bounded invocations test manual single-suite pins against an
existing OpenSSL protocol policy. Add `--check-tls-policy min13` or
`--check-tls-policy max12`, using a separate new output directory for each.
The fixture passes an owned policy file through `OPENSSL_CONF` to a **fresh
candidate daemon child**. It does not change the parent's SSL settings, system
configuration or another daemon.

Each policy mode first verifies that the allowed TLS version, certificate and
authentication work. It then tests the default cipher, a custom list and an
exact single suite against a provider outside the policy's version range. None
may establish a TLS session outside that range. The two controls retain the
existing default/list behavior; the single suite catches a pin overriding the
session's minimum or maximum version. Configuration bytes must remain identical.

These modes use the existing manual `testserver` RPC, which asks for a random,
nonexistent article. Only policy-mode synthetic providers allow that exact
numeric message-ID pattern and return `430 Article not found`, never content.
This permits a real success control without fetching an article. The ordinary
optimizer exercise still rejects every ARTICLE/BODY/GROUP request.
