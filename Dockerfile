FROM oven/bun:1

RUN apt-get update && apt-get install -y git

WORKDIR /home/node/app

COPY package.json ./
RUN bun install --production
COPY . .

CMD [ "bun", "index.js" ]
