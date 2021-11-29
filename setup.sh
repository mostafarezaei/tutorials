#!/bin/bash
# This is a simple bash script for the TUT Application Infrastructure deployment. 
# It basically glues together the parts running in loose coupeling during the deployment and helps to speed things up which
# otherwise would have to be noted down and put into the command line. 
# This can be migrated into real orchestration / automation toolsets if needed (e.g. Ansible, Puppet or Terraform)

# created by Mostafa Rezaei - mostafa.ezaeir@gmail.com
# Disclaimber: NOT FOR PRODUCTION USE - Only for demo and testing purposes

ERROR_COUNT=0; 

if [[ $# -lt 2 ]] ; then
    echo 'arguments missing, please provide at least the aws profile string (-p) and the deployment Stack Name (-s)'
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
  echo 'ERROR: aws cli is not installed.' >&2
  exit 1
fi

if ! docker ps -q 2>/dev/null; then
 echo "ERROR: Docker is not running. Please start the docker runtime on your system and try again"
 exit 1
fi

echo "using AWS Profile $TUTPROFILE"
echo "##################################################"

echo "Validating AWS CloudFormation templates..."
echo "##################################################"
# Loop through the YAML templates in this repository
for TEMPLATE in $(find . -name 'aws-*.template.yaml'); do 

    # Validate the template with CloudFormation
    ERRORS=$(aws cloudformation validate-template --profile=$TUTPROFILE --template-body file://$TEMPLATE 2>&1 >/dev/null); 
    if [ "$?" -gt "0" ]; then 
        ((ERROR_COUNT++));
        echo "[fail] $TEMPLATE: $ERRORS";
    else 
        echo "[pass] $TEMPLATE";
    fi; 
    
done; 

# Error out if templates are not validate. 
echo "$ERROR_COUNT template validation error(s)"; 
if [ "$ERROR_COUNT" -gt 0 ]; 
    then exit 1; 
fi

echo "##################################################"
echo "Validating of AWS CloudFormation templates finished"
echo "##################################################"

# Deploy the Needed Buckets for the later build 
echo "deploy the Prerequisites of the TUT Environment if needed"
echo "##################################################"
TUTPREPSTACK="${TUTSTACK}-Sources"
aws cloudformation deploy --stack-name $TUTPREPSTACK --profile=$TUTPROFILE --template ./templates/aws-buildbuckets.template.yaml
echo "##################################################"
echo "deployment done"

# get the s3 bucket name out of the deployment.
SOURCE=`aws cloudformation describe-stacks --profile=$TUTPROFILE --query "Stacks[0].Outputs[0].OutputValue" --stack-name $TUTPREPSTACK`
SOURCE=`echo "${SOURCE//\"}"`

# we will upload the needed CFN Templates to S3 containing the IaaC Code which deploys the actual infrastructure.
# This will error out if the source files are missing. 
echo "##################################################"
echo "Copy Files to the S3 Bucket for further usage"
echo "##################################################"
if [ -e . ]
then
    echo "##################################################"
    echo "copy TUT code source file"
    aws s3 sync --profile=$TUTPROFILE --exclude=".DS_Store" ./templates s3://$SOURCE
    echo "##################################################"
else
    echo "TUT code source file missing"
    echo "##################################################"
    exit 1
fi
echo "##################################################"
echo "File Copy finished"

# Setting the dynamic Parameters for the Deployment
PARAMETERS=" TUTStackBucketStack=$TUTSTACK-Sources \
             TUTECRRegistry=$REGISTRY"

# Deploy the TUT infrastructure. 
echo "Building the TUT Environment"
echo "##################################################"
aws cloudformation deploy --profile=$TUTPROFILE --stack-name $TUTSTACK \
    --capabilities CAPABILITY_NAMED_IAM \
    --parameter-overrides $PARAMETERS \
    $(jq -r '.Parameters | to_entries | map("\(.key)=\(.value)") | join(" ")' aws-param.json) \
    --template ./aws-root.template.yaml

echo "##################################################"
echo "Deployment finished"

exit 0