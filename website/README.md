# Worldline website

Landing page for [Worldline](https://github.com/agustinmorantes/worldline). Plain static HTML, CSS and JS: no build step.

```sh
npx serve .     # or: python -m http.server
```

- The download buttons point at `/releases/latest` and, once the repository is public, `main.js` asks the GitHub API for the latest release and links the installer for the visitor's OS directly (`*-setup.exe`/`.msi` on Windows, `.AppImage`/`.deb` on Linux).
- Once the repository is public, the GitHub links show the live star count, and a one-time prompt asks visitors to star the repo after they finish the demo or reach the end of the page (dismissal is remembered in `localStorage`).
- If the repository is renamed, update `REPO` in `main.js` and the `github.com/...` links in `index.html`.
- Colors and type follow the app's design tokens. The whole page is driven by one hue (`--h`), like a game's accent in the app.

## Deploy

`.github/workflows/site.yml` publishes this folder to GitHub Pages on every push to `main` or `dev` that touches `website/`. CI (`ci.yml`) ignores changes here. The repository's Pages source must be set to **GitHub Actions**.
