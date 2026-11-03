-- Outcome of the latest delivery to a webhook, so one that stopped working
-- is visible. `last_delivery_error` is a short description with no URL (the
-- URL is the webhook's secret); NULL together with a set `last_delivery_at`
-- means the delivery succeeded.
ALTER TABLE webhooks ADD COLUMN last_delivery_at TEXT;
ALTER TABLE webhooks ADD COLUMN last_delivery_error TEXT;
