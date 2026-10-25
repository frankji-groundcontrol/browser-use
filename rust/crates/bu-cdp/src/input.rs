//! Shared native form filling and ordinary keyboard input.

use anyhow::{anyhow, Context, Result};
use chromiumoxide::cdp::{
    browser_protocol::dom::{FocusParams, ResolveNodeParams},
    js_protocol::runtime::{CallFunctionOnParams, ReleaseObjectParams},
};

use super::BrowserPage;

impl BrowserPage {
    /// Fills a native date/time input, or returns false for ordinary text fields.
    pub async fn fill_native_backend_node_id(
        &self,
        backend_node_id: i64,
        text: &str,
    ) -> Result<bool> {
        let element = self
            .current_element_by_backend_node_id(backend_node_id)
            .await?;
        self.page
            .execute(
                FocusParams::builder()
                    .backend_node_id(element.backend_node_id)
                    .build(),
            )
            .await
            .with_context(|| format!("failed to focus backend node id {backend_node_id}"))?;
        let resolved = self
            .page
            .execute(
                ResolveNodeParams::builder()
                    .backend_node_id(element.backend_node_id)
                    .build(),
            )
            .await?;
        let object_id = resolved
            .object
            .object_id
            .clone()
            .context("input element has no remote object")?;
        let value = serde_json::to_string(text)?;
        let call = CallFunctionOnParams::builder()
            .object_id(object_id.clone())
            .function_declaration(format!(r#"function() {{
                if (this.tagName !== 'INPUT' || !['date', 'time', 'datetime-local', 'month', 'week'].includes(this.type))
                    return {{ handled: false }};
                if (this.disabled || this.readOnly)
                    return {{ handled: true, error: 'input is disabled or read-only' }};
                const value = {value};
                const view = this.ownerDocument.defaultView;
                const setter = Object.getOwnPropertyDescriptor(view.HTMLInputElement.prototype, 'value').set;
                const probe = this.cloneNode(false);
                setter.call(probe, value);
                if (value !== '' && probe.value === '')
                    return {{ handled: true, error: 'invalid native input value' }};
                setter.call(this, value);
                this.dispatchEvent(new view.Event('input', {{ bubbles: true }}));
                this.dispatchEvent(new view.Event('change', {{ bubbles: true }}));
                return this.value === probe.value
                    ? {{ handled: true }}
                    : {{ handled: true, error: 'native input value was reverted' }};
            }}"#))
            .return_by_value(true)
            .build()
            .map_err(|error| anyhow!("failed to build native input call: {error}"))?;
        let result = self.page.execute(call).await;
        let _ = self.page.execute(ReleaseObjectParams::new(object_id)).await;
        let result = result.context("failed to inspect native input")?;
        if result.exception_details.is_some() {
            return Err(anyhow!("native input evaluation failed"));
        }
        let result = result
            .result
            .result
            .value
            .as_ref()
            .context("native input returned no result")?;
        if result["handled"] == true {
            if let Some(error) = result["error"].as_str() {
                return Err(anyhow!("{error}"));
            }
            return Ok(true);
        }
        Ok(false)
    }
}
