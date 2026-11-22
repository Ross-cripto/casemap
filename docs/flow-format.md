# Flows and the map

Each use case is one Markdown file in the catalog (`docs/flows/` by default), and the catalog's `README.md` groups them into a map of user activities.

## What a flow looks like

`docs/flows/check-out-a-cart.md`:

````markdown
# Check out a cart

A customer pays for the items in the cart and gets an order confirmation.

```mermaid
flowchart TD
    subgraph payment [Payment]
        cart([Customer opens the cart]) --> pay[Enter payment details]
        pay --> ok{Payment accepted?}
        error[Show the payment error]
    end
    subgraph confirmation [Confirmation]
        confirm[Show the order confirmation]
    end
    ok -->|yes| confirm
    ok -->|no| error
    error --> pay
    %% @icon confirm check
    %% @icon error x
```
````

- One use case per top-level Markdown file, named in kebab-case, with a title,
  a one-sentence description and one Mermaid `flowchart TD` block.
- Start from the actor or trigger as a stadium `([...])`. Use rectangles for
  steps and diamonds for decisions, with every exit labelled.
- Use `subgraph id [Name]` and `end` to group steps into named sections.
  Sections are optional. In a sectioned flow, declare each step in its section
  and put links between sections after the blocks. `casemap check` warns about
  empty sections, nested sections and steps outside every section.
- Keep it to 3 to 15 steps, using the user's words and behaviour verified in code.
- Keep node ids stable: change labels to rename steps. Keep file paths stable,
  because renaming a file reads as removing one flow and adding another.
- Describe the current behaviour. Casemap computes changes from git.

## Step icons

Add `%% @icon <step> <name>` inside the Mermaid block to choose an icon for a
step. Pick a glyph that helps the reader recognise the action, such as `search`
for a lookup or `file` for opening a document. Casemap defaults to `user` for the
actor, `circle-dot` for a step and `diamond` for a decision. `casemap check` warns
about unknown icon names and step IDs.

Use one of the following names.

`arrow-right`, `bell`, `bot`, `calendar`, `chart-bar`, `check`, `circle-dot`,
`circle-help`, `clock`, `code`, `credit-card`, `database`, `diamond`, `download`,
`eye`, `file`, `file-text`, `filter`, `folder-open`, `git-branch`, `git-compare`,
`globe`, `house`, `image`, `info`, `key-round`, `link`, `list`, `lock`, `log-in`,
`log-out`, `mail`, `map`, `message-square`, `monitor`, `package`, `pencil`, `plus`,
`refresh-cw`, `scan`, `search`, `send`, `settings`, `share-2`, `shopping-cart`,
`split`, `square`, `star`, `tag`, `terminal`, `trash-2`, `triangle-alert`,
`upload`, `user`, `users`, `webhook`, `x`.

## The map

The catalog's `README.md` groups use cases under the user's activities, in
journey order. GitHub renders it when you open the catalog folder:

```markdown
# Shop use-case map

What shoppers do, from finding a product to paying.

## Find products

A shopper looks for something to buy.

- [Browse the storefront](browse-the-storefront.md)
- [Search for products](search-for-products.md)

## Choose and buy

A shopper picks a product and pays.

- [Check out a cart](check-out-a-cart.md)
```

Use `##` headings for activities and flow titles as link text. Each flow belongs
to exactly one activity. `casemap check` reports unknown or duplicate links,
missing flows and mismatched link titles. Ask your coding agent to write and
maintain the map; an ordinary README without activity links stays an ordinary
README.

In an interactive terminal, `casemap map` opens the map in browse. Redirect its
output or pass `--layout` to print the map. The example shows a grid at 80
columns.

```text
 docs/flows · 9 use cases in 3 activities

 ╭─ Find products ──────╮   ╭─ Choose and buy ─────╮   ╭─ Sync the store ─────╮
 │ 4 use cases          │ ▶ │ 4 use cases          │ ▶ │ 1 use case           │
 ╰──────────────────────╯   ╰──────────────────────╯   ╰──────────────────────╯
 · Browse the storefront    · View a product and       · Refresh the catalogue
 · Browse a collection        choose options             after a store edit
 · Search for products      · Add an item to the cart
 · Read a store page        · Manage the cart
                            · Check out a cart
```

When the grid does not fit, the map becomes an outline with each activity's
sentence and its flows. `--layout grid` or `--layout outline` forces a layout.
Width comes from a positive `COLUMNS`, otherwise the stdout terminal when
available, otherwise 80.

`casemap flows` and `casemap browse` use the same activity order, with flows not
on the map last. Browse search keeps the headings of matching groups, and the
diagram header shows the activity and the flow's position within it. Without a
map, both commands keep their flat lists.

## Catalog files

Put each flow in a top-level `.md` file and run `casemap check --strict` to
validate the catalog.
