use super::Engine;
use crate::{config_edit, secrets::Secrets};
use mork_types::config::view::ConfigChange;
use mork_types::protocol::{ErrorBody, ErrorCode};
use serde_json::{Value, json};

pub(super) fn invalid_config_command(cmd: &Value, invalid: mork_types::catalog::Invalid, secrets: &Secrets) -> String {
    let masked = secrets.mask_value(cmd);
    // Serde can escape an offending value, so redact the input before it formats the diagnostic.
    let message = if masked == *cmd {
        invalid.message
    } else {
        mork_types::Command::parse(&masked, false)
            .err()
            .map_or_else(|| format!("Invalid field: {}", invalid.field), |e| e.message)
    };
    secrets.mask(&message)
}

impl Engine {
    pub(super) fn settings_secrets(&self) -> Secrets {
        Secrets::gather(&self.opts.home, &self.ctx.config, &self.ctx.telemetry)
    }

    pub(super) fn config_answer(&self) -> Result<Value, ErrorBody> {
        let secrets = self.settings_secrets();
        let stale =
            std::fs::read_to_string(&self.opts.config_path).ok().as_deref() != Some(self.loaded_config.as_str());
        Ok(json!({"config":config_edit::read(&self.ctx.config,&secrets,stale)}))
    }

    pub(super) fn config_change(&mut self, change: &ConfigChange) -> Result<Value, ErrorBody> {
        let secrets = self.settings_secrets();
        let (text, mut cfg) =
            config_edit::save(&self.opts.config_path, &self.loaded_config, &self.ctx.config, change, &secrets)
                .map_err(|msg| ErrorBody::new(ErrorCode::Refused, secrets.mask(&msg)))?;
        cfg.mork.web_port = self.bound_web_port.or(cfg.mork.web_port);
        self.routing.configure(cfg.routing_enabled());
        self.ctx.config = cfg;
        self.loaded_config = text;
        self.refresh_routing(crate::clock::now_ms());
        self.pipe.slots.freed = true;
        self.publish_screen();
        Ok(json!({"config.change":{}}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parse_refusals_explain_the_invalid_input_and_mask_secrets() {
        let mut secrets = Secrets::default();
        let token = "synthetic-\"token";
        secrets.add("test", token);
        let refusal = |read| {
            let cmd = json!({"config.change":{"change":{"section":"reach", "fields":{
                "domains":{"read":read, "value":{"entries":[]}}
            }}}});
            let invalid = mork_types::Command::parse(&cmd, false).unwrap_err();
            invalid_config_command(&cmd, invalid, &secrets)
        };
        assert!(refusal(json!({"entries":[3]})).contains("expected a string"));
        for read in [json!({token:[]}), json!({"entries":token})] {
            let masked = refusal(read);
            assert!(masked.contains("‹secret›"));
            assert!(!masked.contains("synthetic"));
        }
    }
}
