use esp_idf_svc::sys;
use std::convert::TryFrom;

pub struct ServerTlsConfig {
    certificate: Vec<u8>,
    private_key: Vec<u8>,
    config: sys::esp_tls_cfg_server_t,
}

impl ServerTlsConfig {
    pub fn load(
        certificate_path: &str,
        private_key_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut certificate = std::fs::read(certificate_path)?;
        let mut private_key = std::fs::read(private_key_path)?;

        // ESP-TLS requires null-terminated PEM buffers, including the null byte in their sizes.
        certificate.push(0);
        private_key.push(0);

        let certificate_bytes = u32::try_from(certificate.len())?;
        let private_key_bytes = u32::try_from(private_key.len())?;

        let mut config = sys::esp_tls_cfg_server_t::default();
        config.__bindgen_anon_3 = sys::esp_tls_cfg_server__bindgen_ty_3 {
            servercert_buf: certificate.as_ptr(),
        };
        config.__bindgen_anon_4 = sys::esp_tls_cfg_server__bindgen_ty_4 {
            servercert_bytes: certificate_bytes,
        };
        config.__bindgen_anon_5 = sys::esp_tls_cfg_server__bindgen_ty_5 {
            serverkey_buf: private_key.as_ptr(),
        };
        config.__bindgen_anon_6 = sys::esp_tls_cfg_server__bindgen_ty_6 {
            serverkey_bytes: private_key_bytes,
        };

        Ok(Self {
            certificate,
            private_key,
            config,
        })
    }

    pub fn copy_config(&self) -> sys::esp_tls_cfg_server_t {
        self.config
    }

    pub fn as_mut_ptr(&mut self) -> *mut sys::esp_tls_cfg_server_t {
        // Keep the PEM buffers alive while ESP-TLS uses their pointers.
        let _keep_buffers_alive = (&self.certificate, &self.private_key);
        &mut self.config
    }
}
