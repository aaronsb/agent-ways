---
description: error handling patterns, exception management, try-catch boundaries, error wrapping and propagation
vocabulary: exception handling catch throw boundary wrap rethrow fallback graceful recovery propagate unhandled
pattern: error.?handl|exception|try.?catch|throw|catch
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

## Do Not

- Swallow errors silently (`catch (e) {}`)
- Log the same error at multiple levels — log once at the boundary
- Catch errors you can't handle just to re-throw unchanged

## See Also

- code/security(softwaredev) — error messages can leak information
- code/testing(softwaredev) — test error paths explicitly
