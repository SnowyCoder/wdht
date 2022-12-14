# example_chat
This module is a simple TypeScript chat that uses WebDHT only as a library.

## Buildng
This module does not use Rust, thus it requires a different building environment from the rest of the project.
We use yarn/npm as the dependency manager and [vite](https://vitejs.dev/) as the packager.

First install dependencies by running `yarn`. Then you can use `yarn dev` to create a live server with automatic reloading or `yarn build` to build the final application.

This application is a static site hosted by GitHub pages and it uses a public WebDHT boostrap server.
You can visit the live page [here](https://wdhtchat.rossilorenzo.dev/)

## Deploying
This app is deployed to GitHub static pages by a custom workflow that builds every commit in `main` and pushes the compiled static files to the `gh-pages` branch, that gets published directly.
