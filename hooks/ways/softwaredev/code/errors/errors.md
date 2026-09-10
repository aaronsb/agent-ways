---
description: error handling — exceptions, try-catch boundaries, wrapping and propagation; a missing required config value fails at startup rather than getting a default that silences it; input outside the domain is rejected, not clamped or coerced
vocabulary: exception catch throw boundary wrap rethrow propagate unhandled fallback missing required config env var add a default silence startup fail fast clamp coerce truncate out of range invalid reject validate
pattern: error.?handl|try.?catch|throw
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Error Handling Way

## Where to Catch

Catch at **system boundaries** only — API endpoints, CLI entry points, message handlers. Not inside business logic.

```javascript
// Boundary catch: translate and log
app.get('/users/:id', async (req, res) => {
  try {
    const user = await getUser(req.params.id);
    res.json(user);
  } catch (err) {
    logger.error('getUser failed', { userId: req.params.id, error: err.message });
    res.status(500).json({ error: { code: 'INTERNAL', message: 'Failed to fetch user' } });
  }
});
```

## Wrapping with Context

When crossing module boundaries, add context and re-throw:

```javascript
async function processOrder(orderId) {
  try {
    await chargePayment(orderId);
  } catch (err) {
    throw new Error(`Failed to process order ${orderId}: ${err.message}`, { cause: err });
  }
}
```

## Programmer Errors vs Operational Errors

- **Programmer errors** (bugs): null reference, type mismatch, assertion failure — fail fast, don't catch
- **Operational errors** (expected failures): network timeout, file not found, invalid input — handle gracefully: retry, return fallback, or return clear error to user

## At the Boundary

A required config value that is missing or malformed fails at startup, with a non-zero exit naming every offender. A default added to silence that failure turns a loud deploy failure into a silent wrong value in production. An interpolation that substitutes an empty string is the same defect. Optional means the program works without the value, and it is declared where the value is read.

Input outside a component's domain is refused, and nothing is persisted. Do not round, clamp, truncate, coerce, or merge it into range. Downstream, a repaired value cannot be told from a measured one. A refusal destroys nothing and the caller can retry. Validate against a schema at the boundary so nothing past it is untrusted. Where unknown fields are ignored for forward compatibility, a test asserts that the fields you rely on arrive with their literal values.

An error names its cause with a type or status that describes what happened. Where an upstream refused, carry its text verbatim. That text tells a permission failure from an input error. A status never claims less acceptance than occurred, or the client retries and re-sends committed work.

## Do Not

- Swallow errors silently (`catch (e) {}`)
- Add a default to make a missing-config failure go away
- Log the same error at multiple levels — log once at the boundary
- Catch errors you can't handle just to re-throw unchanged

## See Also

- code/security(softwaredev) — error messages can leak information
- code/testing(softwaredev) — test error paths explicitly
