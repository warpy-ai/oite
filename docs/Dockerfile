# syntax=docker/dockerfile:1

# Build the static site.
FROM node:lts AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build

# Serve ./out with Caddy.
FROM caddy:2-alpine
COPY --from=build /app/out /srv
COPY <<CADDY /etc/caddy/Caddyfile
:80 {
	root * /srv
	try_files {path} {path}.html
	file_server
	handle_errors {
		rewrite * /404.html
		file_server
	}
}
CADDY
EXPOSE 80
