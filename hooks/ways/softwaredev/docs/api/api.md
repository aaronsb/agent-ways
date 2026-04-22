---
description: designing REST APIs, HTTP endpoints, API versioning, request response structure
vocabulary: endpoint api rest route http status pagination versioning graphql request response header payload crud webhook
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# API Design Way

## Common Claude Omissions

When building API endpoints, don't forget:

- **Pagination on every list endpoint** — add it from day one. Default: `?cursor=X&limit=N`, return `next_cursor` in body. Retrofitting pagination is painful.
- **Consistent error shape** — error responses use the same structure as success:
  ```json
  { "error": { "code": "NOT_FOUND", "message": "User 123 not found" } }
  ```
- **Input validation** — validate request body before processing. Return 400 with specific field errors, not a generic message.
- **404 on nested resources** — `GET /users/123/orders` returns 404 if user 123 doesn't exist, not an empty list.

## Defaults (Override Per-Project)

- URL versioning: `/v1/resources`
- Plural nouns: `/users`, not `/user`
- PUT replaces, PATCH updates, DELETE is idempotent
- 201 for creation, 204 for deletion, 409 for conflicts
