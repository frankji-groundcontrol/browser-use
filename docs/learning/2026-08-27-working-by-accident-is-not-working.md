# An integration that works only because the counterparty is lenient is untested

Date: 2026-08-27 · Scope: protocol clients, gateway integrations, provider abstractions

## The lesson

The Rust port had an "Anthropic" configuration: set `ANTHROPIC_BASE_URL` and
`ANTHROPIC_AUTH_TOKEN`, and it worked. It had worked for months.

It never spoke Anthropic. It POSTed an **OpenAI-shaped body** to
`{base}/chat/completions` with bearer auth. No `x-api-key`, no
`anthropic-version` header, `system` left in the message array, no `max_tokens`.
Pointed at `api.anthropic.com` it would have failed outright. It worked only
because the gateways in use happen to serve both protocols and quietly accepted
the OpenAI shape.

The code even documented its own reasoning — "verified, so the existing client
works unchanged — no Anthropic-native provider is needed". The verification was
real; the conclusion it licensed was not. What was verified was *that one
lenient gateway accepted it*, and that got generalized into *the protocol is
compatible*.

**The rule:** when an option is named after a protocol, test it against that
protocol's own endpoint, not against a translator that happens to sit in front.
"It works in production" is evidence about your counterparty, not about your
client.

## The tell

The giveaway is a **naming/behaviour mismatch you can read without running
anything**: an option called `anthropic` whose code path contains no Anthropic
concepts. Grepping the client for `x-api-key`, `anthropic-version`, or top-level
`system` returned nothing — the abstraction had a name it did not earn.

Cheap check, worth doing whenever a provider option exists: search for the one
header or field that is *unique* to that provider. If it is absent, the option is
a label, not an implementation.

## Corroborating evidence from the same task

Probing the deployed gateway before choosing defaults surfaced something the
config had been getting away with by luck:

| Route | Result |
| --- | --- |
| `/v1/chat/completions` | 200 JSON |
| `/chat/completions` | **200 HTML** — the console page, not an error |

A wrong root does not 404 here; it returns a *success* status with an HTML body.
Anything that only checks the status code would treat the login page as a model
response. Same shape of failure as above: leniency masking a defect.

## When to apply it again

- Adding or reviewing any provider/protocol option, especially behind a shared
  client. Ask what is unique to that protocol, then grep for it.
- Any integration whose confidence rests on "it works against our gateway".
  Gateways normalize; the normalization is doing work you have not tested.
- Treat a 2xx with an unexpected `content-type` as a routing failure, not a
  success. Status alone is not proof of having reached the right endpoint.

## What this cost and what it bought

Nothing broke in production, because the lenient gateway kept absorbing it. That
is precisely why it survived so long — a defect with no symptom gets no
attention. It surfaced only when someone asked for Anthropic support and the
honest answer turned out to be "the existing option is a placeholder".

Related: [changelog](../changelogs/2026-08-27-llm-config-redesign.md),
[plan](../plans/2026-08-27-rust-chatbrowseruse/2026-08-27-rust-chatbrowseruse.md).
