# Shared UI components

The frontend uses [shadcn-svelte](https://www.shadcn-svelte.com/docs), installed from
its official registry with CLI version 1.7.0 and the `nova` style. Configuration lives
in `web/components.json`. Component source is checked into `web/src/lib/components/ui/`;
there is no application-level shadcn runtime or second state container.

## Ownership and usage

Use Button, Input, Textarea, Label, Dialog, Avatar, Badge, Card, NativeSelect and
Progress for common presentation. Business components remain under `lib/features/`.
The shared UI layer must not import features, issue HTTP requests or own timers.
For example, `ProfileAvatar.svelte` still authorizes, downloads and revokes avatar
blob URLs; Avatar primitives only render the resulting image and fallback.

```svelte
<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  let name = $state('');
</script>

<form onsubmit={(event) => event.preventDefault()}>
  <Label for="example-name">Name</Label>
  <Input id="example-name" bind:value={name} required />
  <Button type="submit">Save</Button>
</form>
```

Button defaults to `type="button"`; submitting forms must set `type="submit"`.
Use `bind:ref` for the underlying native element. Keep input/composition handlers,
required attributes, autocomplete, accessible names and disabled conditions on the
feature's control. Use Dialog.Trigger's child snippet to compose a Button without
nesting two native buttons. Dialog.Content includes its own portal, overlay and close
button; pass `closeLabel` with the feature's existing localized accessible name.

## Theme and local adaptations

`ui-theme.css` maps shadcn semantic colors to the existing green product palette.
The theme continues to use `data-theme="light"` or `data-theme="dark"`; no theme
manager is added. The dark variant applies to document-root portal content as well.
Imports load Tailwind, animation utilities and semantic UI tokens before workflow
styles, with final responsive/theme overrides retaining their relative order.

Local component adaptations are intentionally small:

- Button and Input defaults use larger dimensions suitable for the existing forms.
- Avatar defaults use the product's rounded-square shape and semantic colors.
- Dialog.Content uses a 480px desktop maximum, bounded scrolling on small screens,
  and a configurable close label.
- The composer overrides Textarea's minimum height and field sizing to retain its
  existing DOM-driven resize behavior.
- File inputs bind `files` rather than `value`. Reapplying a previously selected
  filename after a feature clears the native chooser is forbidden by browsers.
- ProfileAvatar resets the visual Avatar root when image presence changes, so
  removing a loaded image restores initials without restarting the download effect.

Feature CSS retains layout, message bubbles, read-status display and media geometry.
The previous global button/input/select/dialog/avatar styling has been removed.
Prefer variants and utility overrides over adding a competing global control style.

Reading text uses the shared `foundation.css` scale: `--font-body` (1rem) for
messages and conversation names, `--font-secondary` (0.875rem) for previews and
supporting descriptions, and `--font-caption` (0.75rem) for timestamps and metadata.
Keep message text at the same scale on mobile; adapt available width and spacing
instead of shrinking the content. Chat history and composer share
`--chat-content-width` so their desktop reading areas expand together.

## Adding and updating components

Run from `web/`:

```powershell
pnpm exec shadcn-svelte add tooltip --yes
```

Only add a component when a feature needs it. Review downloaded source and dependency
changes. Do not use `--overwrite` without comparing the existing local adaptations.
CLI versions are pinned in the package manifest and lockfile; component source is
owned by this repository and is not automatically updated by a package upgrade.

Run `node scripts/check.mjs` at the repository root. UI migrations should also run the
existing browser suite, including forms, keyboard composition, dialogs, settings,
groups and media. For deployed changes, check an already deployed HTTPS staging
application with
`pnpm -C web test:deployment`. Configure its origins and invitations using
[the deployment guide](../operations/deployment.md); the checks do not build images.
