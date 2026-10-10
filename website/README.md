# Worldline website

Landing page for [Worldline](https://github.com/agustinmorantes/worldline). Plain static HTML, CSS and JS: no build step.

```sh
npx serve .     # or: python -m http.server
```

- The download buttons point at `/releases/latest` and, once the repository is public, `main.js` asks the GitHub API for the latest release and links the installer for the visitor's OS directly (`*-setup.exe`/`.msi` on Windows, `.AppImage`/`.deb` on Linux).
- If the repository is renamed, update `REPO` in `main.js` and the `github.com/...` links in `index.html`.
- Colors and type follow the app's design tokens. The whole page is driven by one hue (`--h`), like a game's accent in the app.

Deploys as-is to GitHub Pages, Netlify, Vercel or any static host.
