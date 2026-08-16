FROM jhao104/proxy_pool:2.4.2

WORKDIR /app

COPY validator.patch /app/validator.patch
RUN apk add --no-cache patch
RUN patch -p1 < validator.patch

# The patched validator now lives at /app/helper/validator.py
