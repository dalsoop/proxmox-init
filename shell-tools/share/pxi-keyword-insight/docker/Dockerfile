FROM mcr.microsoft.com/playwright:v1.52.0-noble

WORKDIR /app

COPY package.json package-lock.json* ./
RUN npm ci --omit=dev

COPY server.js ./
COPY playwright.config.ts ./

ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright
ENV PORT=3000

EXPOSE 3000
ENTRYPOINT ["node", "server.js"]
