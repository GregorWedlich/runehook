FROM node:20-alpine

WORKDIR /app
COPY ./api /app 
COPY .git /app/.git

RUN apk add --no-cache --virtual .build-deps git
RUN npm ci --no-audit && \
    npm run build && \
    (npm run generate:git-info || echo 'Skipping git info generation') && \
    npm prune --production
RUN apk del .build-deps

CMD ["sh", "-c", "if [ ! -f .git-info ]; then echo 'Skipping git info generation'; else node ./dist/src/index.js; fi"]
