/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2008-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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

#ifndef TLSSOCKET_H
#define TLSSOCKET_H

#include <chrono>
#include <functional>
#include <cstdint>
#include "nzbget_rs.h"

struct SocketDeadline
{
	std::chrono::steady_clock::time_point end;
	std::function<bool()> cancelled;
	bool Stopped() const { return std::chrono::steady_clock::now() >= end || (cancelled && cancelled()); }
	bool Wait(SOCKET socket, bool read, bool write) const
	{
		return nzbget_rs_socket_deadline_wait(
			(int)socket,
			read ? 1 : 0,
			write ? 1 : 0,
			const_cast<SocketDeadline*>(this),
			+[](void* ctx) noexcept -> int64_t
			{
				const auto* self = static_cast<const SocketDeadline*>(ctx);
				if (self->Stopped()) return -1;
				return std::chrono::duration_cast<std::chrono::milliseconds>(
					self->end - std::chrono::steady_clock::now()).count();
			}) != 0;
	}
};

#ifndef DISABLE_TLS

#include <string_view>
#include <openssl/sslerr.h>
#include "Log.h"
#include "Util.h"
#include "Options.h"

class TlsSocket
{
public:
	TlsSocket(
		SOCKET socket,
		bool isClient,
		const char* host,
		const char* certFile,
		const char* keyFile,
		const char* cipher,
		int certVerifLevel)
		: m_socket(socket)
		, m_host(host ? host : "")
		, m_certFile(certFile ? certFile : "")
		, m_keyFile(keyFile ? keyFile : "")
		, m_cipher(cipher ? cipher : "")
		, m_isClient(isClient)
		, m_retCode(0)
		, m_certVerifLevel(certVerifLevel)
		, m_context{ nullptr, &SSL_CTX_free }
		, m_session{ nullptr, &SSL_free } {}

	virtual ~TlsSocket()
	{
		Close();
		ERR_clear_error();
	}

	static void Init()
	{
		debug("Initializing TLS library");

		if (OPENSSL_init_ssl(0, nullptr) == 0)
		{
			error("Could not initialize the OpenSSL library");
		}
	}

	static void InitOptions(const char* certStore)
	{
		if (Util::EmptyStr(certStore)) 
			return;

		InitX509Store(certStore);
	}

	static void Final()
	{
		m_X509Store.reset();
		OPENSSL_cleanup();
	}

	bool Start()
	{
		m_context.reset(SSL_CTX_new(TLS_method()));

		if (!m_context)
		{
			ReportError("Could not create TLS context", false);
			return false;
		}

		if (nzbget_rs_dqh_tls_has_cert_and_key_files(
			m_certFile.empty() ? 1 : 0, m_keyFile.empty() ? 1 : 0) != 0)
		{
			if (!SSL_CTX_use_certificate_chain_file(m_context.get(), m_certFile.c_str()))
			{
				ReportError("Could not load certificate file", false);
				Close();
				return false;
			}
			if (!SSL_CTX_use_PrivateKey_file(m_context.get(), m_keyFile.c_str(), SSL_FILETYPE_PEM))
			{
				ReportError("Could not load key file", false);
				Close();
				return false;
			}
		}

		if (nzbget_rs_dqh_tls_should_verify_client_x509_store(
			m_isClient ? 1 : 0, m_X509Store ? 1 : 0) != 0)
		{
			SSL_CTX_set1_cert_store(m_context.get(), m_X509Store.get());

			if (nzbget_rs_ntw_tls_should_verify_peer(
				1, (int)m_certVerifLevel, (int)Options::ECertVerifLevel::cvNone) != 0)
			{
				SSL_CTX_set_verify(m_context.get(), SSL_VERIFY_PEER, nullptr);
			}
			else
			{
				SSL_CTX_set_verify(m_context.get(), SSL_VERIFY_NONE, nullptr);
			}
		}

		m_session.reset(SSL_new(m_context.get()));
		if (!m_session)
		{
			ReportError("Could not create TLS session", false);
			Close();
			return false;
		}

		if (!SetCipherSuite(m_cipher))
		{
			ReportError("Could not set cipher suite for SSL/TLS connection", false);
			Close();
			return false;
		}

		if (nzbget_rs_dqh_tls_should_set_sni_hostname(
			m_isClient ? 1 : 0, m_host.empty() ? 1 : 0) != 0 &&
			!SSL_set_tlsext_host_name(m_session.get(), m_host.c_str()))
		{
			ReportError("Could not set host name for TLS");
			Close();
			return false;
		}

		if (!SSL_set_fd(m_session.get(), static_cast<int>(m_socket)))
		{
			ReportError("Could not set the file descriptor for TLS");
			Close();
			return false;
		}

		int ec;
		do
		{
			if (m_deadline && m_deadline->Stopped()) { Close(); return false; }
			ec = m_isClient ? SSL_connect(m_session.get()) : SSL_accept(m_session.get());
		} while (ec < 1 && RetryIo(ec));
		bool shouldVerify = nzbget_rs_ntw_tls_should_verify_peer(
			m_X509Store ? 1 : 0, (int)m_certVerifLevel, (int)Options::ECertVerifLevel::cvNone) != 0;
		if (ec < 1)
		{
			if (m_deadline)
			{
				unsigned long code = ERR_peek_last_error();
				int reason = ERR_GET_REASON(code);
				if (nzbget_rs_ntw_tls_is_explicit_handshake_rejection(
					ERR_GET_LIB(code), ERR_LIB_SSL, reason,
					SSL_R_SSLV3_ALERT_HANDSHAKE_FAILURE,
					SSL_R_TLSV1_ALERT_PROTOCOL_VERSION,
					SSL_R_NO_SHARED_CIPHER,
					SSL_R_UNSUPPORTED_PROTOCOL) != 0)
					m_probeFailure = 0; // Explicit rejection of this single-suite handshake.
			}
			if (nzbget_rs_dqh_tls_should_check_verify_result_on_handshake_fail(
				m_isClient ? 1 : 0, shouldVerify ? 1 : 0) != 0)
			{
				long verifyRes = SSL_get_verify_result(m_session.get());
				if (verifyRes != X509_V_OK)
				{
					m_probeFailure = -4;
					NzbgetRsBuf verifyErrBuf = nzbget_rs_dqh_tls_format_cert_verify_err(
						m_host.c_str(), X509_verify_cert_error_string(verifyRes));
					PrintError(verifyErrBuf.data ? verifyErrBuf.data : "");
					nzbget_rs_free(verifyErrBuf);
					Close();
					return false;
				}
			}

			NzbgetRsBuf hsErrBuf = nzbget_rs_dqh_tls_format_handshake_err(m_host.c_str());
			ReportError(hsErrBuf.data ? hsErrBuf.data : "");
			nzbget_rs_free(hsErrBuf);
			Close();
			return false;
		}

		if (nzbget_rs_dqh_tls_should_validate_client_cert(
			m_isClient ? 1 : 0, shouldVerify ? 1 : 0) != 0 && !ValidateCert())
		{
			m_probeFailure = -4;
			Close();
			return false;
		}

		const SSL_CIPHER* currentCipher = SSL_get_current_cipher(m_session.get());
		if (currentCipher != nullptr)
		{
			debug("TLS connection established: %s using %s", SSL_get_version(m_session.get()), SSL_CIPHER_get_name(currentCipher));
		}

		m_connected = true;
		return true;
	}

	void Close()
	{
		if (nzbget_rs_dqh_tls_should_send_close_notify(
			m_session ? 1 : 0, m_connected ? 1 : 0, m_deadline ? 1 : 0) != 0)
		{
			if (SSL_shutdown(m_session.get()) == 0)
			{
				// The bidirectional shutdown is not yet complete. Call SSL_shutdown again.
				SSL_shutdown(m_session.get());
			}
		}

		m_context.reset();
		m_session.reset();
	}

	int Send(const char* buffer, int size)
	{
		do
		{
			if (m_deadline && m_deadline->Stopped()) return -1;
			m_retCode = SSL_write(m_session.get(), buffer, size);
		} while (m_retCode <= 0 && RetryIo(m_retCode));

		if (m_retCode < 0)
		{
			ReportError(nzbget_rs_dqh_tls_select_io_err_msg(1, ERR_peek_error() == 0 ? 1 : 0));
			return -1;
		}

		return m_retCode;
	}

	int Recv(char* buffer, int size)
	{
		do
		{
			if (m_deadline && m_deadline->Stopped()) return -1;
			m_retCode = SSL_read(m_session.get(), buffer, size);
		} while (m_retCode <= 0 && RetryIo(m_retCode));

		if (m_retCode < 0)
		{
			ReportError(nzbget_rs_dqh_tls_select_io_err_msg(0, ERR_peek_error() == 0 ? 1 : 0));
			return -1;
		}

		return m_retCode;
	}

	void SetSuppressErrors(bool suppressErrors) { m_suppressErrors = suppressErrors; }
	const char* GetNegotiatedCipher() const { return m_session ? SSL_get_cipher_name(m_session.get()) : ""; }
	int GetNegotiatedVersion() const { return m_session ? SSL_version(m_session.get()) : 0; }
	static bool HasCertificateStore() { return static_cast<bool>(m_X509Store); }
	int GetProbeFailure() const { return m_probeFailure; }
	void SetDeadline(std::shared_ptr<const SocketDeadline> deadline) { m_deadline = std::move(deadline); }

protected:
	virtual void PrintError(const char* errMsg)
	{
		error("%s", errMsg);
	}

private:
	static void InitX509Store(std::string_view certStore)
	{
		m_X509Store.reset(X509_STORE_new());

		if (!m_X509Store)
		{
			error("Could not create certificate store");
			return;
		}

		if (!X509_STORE_load_locations(m_X509Store.get(), certStore.data(), nullptr))
		{
			error("Could not load certificate store location from %s", certStore.data());
			return;
		}
	}

	inline static OpenSSL::X509StorePtr m_X509Store{ nullptr, &X509_STORE_free };

	/**
	 * This function configures the allowed ciphers for both TLSv1.3 and
	 * older protocols from a single, combined string. OpenSSL
	 * will parse the string and apply the relevant ciphers to their
	 * respective protocol versions.
	 *
	 * * @note This implementation allows a mixed list, but TLSv1.3 ciphers will
	 * always be preferred during the handshake if the server supports them,
	 * regardless of their order in the string.
	 */
	bool SetCipherSuite(std::string_view cipher)
	{
		if (cipher.empty())
		{
			return true;
		}

		// A single modern suite is an exact pin. Without a protocol bound, setting
		// only a TLS 1.2 list leaves the default TLS 1.3 suites usable (and vice versa).
		// Empty values and custom lists retain their historical negotiation behavior.
		if (!nzbget_rs_dqh_tls_is_cipher_list(cipher.data(), cipher.size()))
		{
			const auto* ciphers = SSL_get_ciphers(m_session.get());
			for (int i = 0; i < sk_SSL_CIPHER_num(ciphers); ++i)
			{
				const SSL_CIPHER* candidate = sk_SSL_CIPHER_value(ciphers, i);
				const char* candName = SSL_CIPHER_get_name(candidate);
				if (cipher != candName) continue;
				bool tls13 = nzbget_rs_dqh_tls_is_tls13_version_str(SSL_CIPHER_get_version(candidate)) != 0;
				int auth = SSL_CIPHER_get_auth_nid(candidate), kx = SSL_CIPHER_get_kx_nid(candidate);
				unsigned flags = nzbget_rs_dqh_tls_compute_suite_flags(
					SSL_CIPHER_is_aead(candidate) ? 1 : 0,
					tls13 ? 1 : 0,
					(auth == NID_auth_rsa || auth == NID_auth_ecdsa) ? 1 : 0,
					(kx == NID_kx_ecdhe || kx == NID_kx_dhe) ? 1 : 0);
				unsigned bits = nzbget_rs_dqh_tls_resolve_cipher_bits(
					candName, (unsigned)SSL_CIPHER_get_bits(candidate, nullptr));
				if (!nzbget_rs_tls_cipher_pin(tls13 ? 13 : 12, bits, flags)) break;
				int version = tls13 ? TLS1_3_VERSION : TLS1_2_VERSION;
				int minimum = SSL_get_min_proto_version(m_session.get());
				int maximum = SSL_get_max_proto_version(m_session.get());
				if (nzbget_rs_dqh_tls_is_proto_version_out_of_bounds(version, minimum, maximum) != 0) return false;
				if (!SSL_set_min_proto_version(m_session.get(), version) || !SSL_set_max_proto_version(m_session.get(), version)) return false;
				return tls13 ? SSL_set_ciphersuites(m_session.get(), m_cipher.c_str()) == 1 :
					SSL_set_cipher_list(m_session.get(), m_cipher.c_str()) == 1;
			}
		}

		bool tls13 = SSL_set_ciphersuites(m_session.get(), cipher.data());
		bool legacy = SSL_set_cipher_list(m_session.get(), cipher.data());

		if (!tls13)
			ERR_clear_error();

		return tls13 || legacy;
	}

	SOCKET m_socket;

	std::string m_host;
	std::string m_certFile;
	std::string m_keyFile;
	std::string m_cipher;

	bool m_isClient;
	bool m_suppressErrors = false;
	bool m_connected = false;
	int m_probeFailure = -1;

	int m_retCode;
	int m_certVerifLevel;

	OpenSSL::SSLCtxPtr m_context;
	OpenSSL::SSLSessionPtr m_session;
	std::shared_ptr<const SocketDeadline> m_deadline;

	bool RetryIo(int result)
	{
		if (!m_deadline) return false;
		int error = SSL_get_error(m_session.get(), result);
		int wantRead = 0;
		int wantWrite = 0;
		if (!nzbget_rs_ntw_tls_classify_want_io(
			error, SSL_ERROR_WANT_READ, SSL_ERROR_WANT_WRITE, &wantRead, &wantWrite))
			return false;
		return m_deadline->Wait(m_socket, wantRead != 0, wantWrite != 0);
	}

	void ReportError(const char* errMsg, bool suppressable = true)
	{
		auto errcode = ERR_get_error();
		do
		{
			char errstr[1024];
			ERR_error_string_n(errcode, errstr, sizeof(errstr));
			errstr[1024 - 1] = '\0';

			if (suppressable && m_suppressErrors)
			{
				NzbgetRsBuf formattedErr = nzbget_rs_dqh_tls_format_report_err(errMsg, errstr);
				debug("%s", formattedErr.data ? formattedErr.data : "");
				nzbget_rs_free(formattedErr);
			}
			else if (errcode != 0)
			{
				NzbgetRsBuf formattedErr = nzbget_rs_dqh_tls_format_report_err(errMsg, errstr);
				PrintError(formattedErr.data ? formattedErr.data : "");
				nzbget_rs_free(formattedErr);
			}
			else
			{
				PrintError(errMsg);
			}

			errcode = ERR_get_error();
		} while (errcode);
	}

	bool ValidateCert()
	{
		// verify a server certificate was presented during the negotiation
		OpenSSL::X509Ptr cert{ SSL_get_peer_certificate(m_session.get()), &X509_free };
		if (!cert)
		{
			NzbgetRsBuf verifyErrBuf = nzbget_rs_dqh_tls_format_cert_verify_err(
				m_host.c_str(), "no certificate provided by server");
			PrintError(verifyErrBuf.data ? verifyErrBuf.data : "");
			nzbget_rs_free(verifyErrBuf);
			return false;
		}

#ifdef HAVE_X509_CHECK_HOST
		// hostname verification
		bool shouldCheckHost = nzbget_rs_ntw_tls_should_check_host(
			(int)m_certVerifLevel, (int)Options::ECertVerifLevel::cvMinimal, m_host.empty() ? 1 : 0) != 0;
		if (shouldCheckHost && !X509_check_host(cert.get(), m_host.c_str(), m_host.size(), 0, nullptr))
		{
			const unsigned char* certHost = nullptr;
			// Find the position of the CN field in the Subject field of the certificate
			int common_name_loc = X509_NAME_get_index_by_NID(X509_get_subject_name(cert.get()), NID_commonName, -1);
			if (common_name_loc >= 0)
			{
				// Extract the CN field
				const X509_NAME_ENTRY* common_name_entry = X509_NAME_get_entry(X509_get_subject_name(cert.get()), common_name_loc);
				if (common_name_entry != nullptr)
				{
					// Convert the CN field to a C string
					const ASN1_STRING* common_name_asn1 = X509_NAME_ENTRY_get_data(common_name_entry);
					if (common_name_asn1 != nullptr)
					{
						certHost = ASN1_STRING_get0_data(common_name_asn1);
					}
				}
			}

			NzbgetRsBuf mismatchErrBuf = nzbget_rs_dqh_tls_format_hostname_mismatch_err(
				m_host.c_str(), reinterpret_cast<const char*>(certHost));
			PrintError(mismatchErrBuf.data ? mismatchErrBuf.data : "");
			nzbget_rs_free(mismatchErrBuf);
			return false;
		}
#endif

		return true;
	}
};

#endif
#endif
