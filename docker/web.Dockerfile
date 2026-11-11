# syntax=docker/dockerfile:1
# フロントエンド。ビルド済みの React を nginx で配信し、/api と /ws を server へ中継する。

FROM node:22.22.3-alpine3.23 AS build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

FROM nginx:1.30.4-alpine
COPY docker/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /web/dist /usr/share/nginx/html
EXPOSE 80
