#!/bin/sh
set -eu

# Sandbox bucket initialization; authentication/network errors must stop startup.
s3() {
  aws --endpoint-url http://rustfs:9000 s3api "$@"
}

bucket=$(s3 list-buckets --query "Buckets[?Name=='molesignal'].Name | [0]" --output text)
if [ "$bucket" = "None" ]; then
  s3 create-bucket --bucket molesignal
fi

# The bucket stays private: MoleSignal serves avatars and every other object itself.
# Remove any bucket policy that an existing volume already carries.
if s3 get-bucket-policy --bucket molesignal >/dev/null 2>&1; then
  s3 delete-bucket-policy --bucket molesignal
fi
echo 'bucket ready'
