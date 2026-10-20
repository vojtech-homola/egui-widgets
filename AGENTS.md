# Agent instructions

- Changes reach `main` either as direct commits or as squash-merged PRs.
- Direct commits to `main` must use [Conventional Commits](https://www.conventionalcommits.org/) messages.
- PR titles must use Conventional Commits, because the title becomes the squash commit message.
- Mark breaking changes with `!` (e.g. `feat!: update to egui 0.37`).
- Don't edit `CHANGELOG.md` by hand; it is generated from commit messages by git-cliff.
