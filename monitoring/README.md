# Monitoring Runbooks

Operational triage procedures for alerts defined under `grafana/provisioning/alerting/`.

Each alert rule sets a `runbook_url` pointing to this document. When an alert fires,
follow the matching section below.

## Uptime Alerts

Source: `grafana/provisioning/alerting/uptime-alerts.yaml`

### General triage flow

1. Confirm the alert is real — check the Grafana alert panel and the target's status page.
2. Identify the affected service and environment (staging vs. production).
3. Check recent deploys, config changes, and infrastructure events.
4. Escalate to the owning team if the service does not recover within the alert's `for` window.
5. Record the incident timeline and resolution in the incident channel.

### Service down / probe failing

- Verify the endpoint manually (`curl -sS -o /dev/null -w '%{http_code}' <url>`).
- Check DNS resolution and TLS certificate validity.
- Inspect the service logs and recent deployments.
- If the service is down, follow the standard rollback procedure and notify on-call.

### Elevated error rate

- Inspect application logs for the dominant error class.
- Correlate with recent deploys or dependency outages.
- If errors are caused by a bad release, roll back; otherwise escalate to the service owner.

### High latency

- Check upstream dependencies and database health.
- Review resource saturation (CPU, memory, connections) for the affected service.
- Scale out or shed load if saturation is confirmed.

## Status Page

Status page configuration lives under `monitoring/statuspage/`. Update public status
components when an incident affects customer-facing availability.

## Escalation

- Primary on-call: respond within the alert's configured window.
- Secondary on-call: engage if primary does not acknowledge.
- Include the alert name, affected service, and current impact in every escalation.
