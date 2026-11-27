use super::Engine;
use crate::{config_edit, secrets::Secrets};
use mork_types::config::view::ConfigChange;
use mork_types::protocol::{ErrorBody, ErrorCode};
use serde_json::{Value, json};

impl Engine {
    pub(super) fn config_answer(&self) -> Result<Value, ErrorBody> {
        let secrets = Secrets::gather(&self.opts.home, &self.ctx.config, &self.ctx.telemetry);
        let stale =
            std::fs::read_to_string(&self.opts.config_path).ok().as_deref() != Some(self.loaded_config.as_str());
        Ok(json!({"config":config_edit::read(&self.ctx.config,&secrets,stale)}))
    }

    pub(super) fn config_change(&mut self, change: &ConfigChange) -> Result<Value, ErrorBody> {
        let secrets = Secrets::gather(&self.opts.home, &self.ctx.config, &self.ctx.telemetry);
        let (text, mut cfg) =
            config_edit::save(&self.opts.config_path, &self.loaded_config, &self.ctx.config, change, &secrets)
                .map_err(|msg| ErrorBody::new(ErrorCode::Refused, secrets.mask(&msg)))?;
        cfg.mork.web_port = self.bound_web_port.or(cfg.mork.web_port);
        self.ctx.config = cfg;
        self.loaded_config = text;
        self.pipe.slots.freed = true;
        self.publish_screen();
        Ok(json!({"config.change":{}}))
    }
}
