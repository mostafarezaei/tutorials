#!/bin/bash
# This is a simple bash script for the TUT Application Infrastructure deployment. 
# It basically glues together the parts running in loose coupeling during the deployment and helps to speed things up which
# otherwise would have to be noted down and put into the command line. 
# This can be migrated into real orchestration / automation toolsets if needed (e.g. Ansible, Puppet or Terraform)

# created by Mostafa Rezaei - mostafa.ezaeir@gmail.com
# Disclaimber: NOT FOR PRODUCTION USE - Only for demo and testing purposes

ERROR_COUNT=0; 

if [[ $# -lt 2 ]] ; then
    echo 'arguments missing, please the aws profile string (-p) and the deployment Stack Name (-s)'
    exit 1
fi

while getopts ":p:s:" opt; do
  case $opt in
    p) TUTPROFILE="$OPTARG"
    ;;
    s) TUTSTACK="$OPTARG"
    ;;
    \?) echo "Invalid option -$OPTARG" >&2
    ;;
  esac
done

if ! [ -x "$(command -v aws)" ]; then
  echo 'Error: aws cli is not installed.' >&2
  exit 1
fi

echo "using AWS Profile $TUTPROFILE"
echo "##################################################"

# Destroy the TUT infrastructure. 
echo "Delete the TUT Environment"
echo "##################################################"
aws cloudformation delete-stack --profile=$TUTPROFILE --stack-name $TUTSTACK 

aws cloudformation wait stack-delete-complete --profile=$TUTPROFILE --stack-name $TUTSTACK

echo "##################################################"
echo "Deletion finished"

# Destroy Bucket and ECR 
echo "deleting the Prerequisites stacks"
echo "##################################################"

TUTPREPSTACK="${TUTSTACK}-Sources"
SOURCE=`aws cloudformation describe-stacks --profile=$TUTPROFILE --query "Stacks[0].Outputs[0].OutputValue" --stack-name $TUTPREPSTACK`

SOURCE=`echo "${SOURCE//\"}"`

echo "##################################################"
echo "Truncate S3 Buckets"
echo "##################################################"
aws s3 rb s3://$SOURCE --force 

echo "##################################################"
echo "Truncate and delete the ECR Repositories"
echo "##################################################"

TUTECRStack="${TUTSTACK}-registry"
TUTREGISTRY=`aws cloudformation describe-stacks --profile=$TUTPROFILE --query "Stacks[0].Outputs[0].OutputValue" --stack-name $TUTECRStack`
TUTREGISTRY=`echo "${TUTREGISTRY//\"}"`

IMAGESBLUELIGHT=$(aws --profile $TUTPROFILE ecr describe-images --repository-name $TUTREGISTRY --output json | jq '.[]' | jq '.[]' | jq "select (.imagePushedAt > 0)" | jq -r '.imageDigest')
for IMAGE in ${IMAGESBLUELIGHT[*]}; do
    echo "Deleting $IMAGE"
    aws ecr --profile $TUTPROFILE batch-delete-image --repository-name $TUTREGISTRY --image-ids imageDigest=$IMAGE
done
aws cloudformation delete-stack --profile=$TUTPROFILE --stack-name $TUTECRStack 
aws cloudformation wait stack-delete-complete --profile=$TUTPROFILE --stack-name $TUTECRStack

echo "##################################################"
echo "Deletion done"


exit 0 
