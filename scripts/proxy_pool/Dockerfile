FROM jhao104/proxy_pool:2.4.2 AS patched

WORKDIR /app

COPY validator.patch /app/validator.patch
RUN apk add --no-cache patch
RUN patch -p1 < validator.patch

FROM jhao104/proxy_pool:2.4.2

WORKDIR /app

COPY --from=patched /app/helper/validator.py /app/helper/validator.py
