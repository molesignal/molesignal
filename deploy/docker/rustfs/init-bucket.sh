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

# Preserve the sandbox's public object downloads (including user avatars).
cat > /tmp/bucket-policy.json <<'JSON'
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {"AWS": ["*"]},
      "Action": ["s3:GetBucketLocation", "s3:ListBucket"],
      "Resource": ["arn:aws:s3:::molesignal"]
    },
    {
      "Effect": "Allow",
      "Principal": {"AWS": ["*"]},
      "Action": ["s3:GetObject"],
      "Resource": ["arn:aws:s3:::molesignal/*"]
    }
  ]
}
JSON
s3 put-bucket-policy --bucket molesignal --policy file:///tmp/bucket-policy.json
echo 'bucket ready'
