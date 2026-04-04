# Return to Default Branch After Local Updates

## Priority: Medium
## Status: Open

## Description

After creating update branches in local git mode, the working tree is left checked out on the last update branch instead of the default branch (main). This is surprising for users who expect the repo to remain on main after reforge runs.

## Current Behavior

```
$ git branch
  main
  reforge/docker-curlimages-curl-8.18.0
  reforge/docker-hashicorp-vault-1.21.4
* reforge/docker-nginx-1.29.7           <-- left here
  reforge/helm-stateless-http-service-14.9.0
```

## Expected Behavior

After all branches/commits are created, reforge should `git checkout <default_branch>` to restore the working tree.

## Acceptance Criteria

- [ ] `apply_local_updates()` returns to `default_branch` after processing all groups
- [ ] Working tree is clean on the default branch when reforge exits
