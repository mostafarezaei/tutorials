# **Tutorial Examples in Three Languages**

Tutorial on how to deploy a product that uses different AWS services.

## Summary

This project shows how to use different AWS services such as ECS, EC2, KMS and S3. Of course, for the simplicity of the part of the tutorial that uses a programming language, the three languages ​​Python, Rust and R are used.

## Disclaimer
This project is an example of an deployment and meant to be used for testing and learning purposes only. Do not use in production.

**Be aware that the deployment is not covered by the AWS free tier. Please use the [AWS pricing calculator](https://calculator.aws/#/estimate) to an estimation beforehand**

# Table of Contents

1. [Getting started](#Getting-started)
2. [Prerequisites](#Prerequisites)
3. [Parameters](#Parameters)
4. [Templates](#Templates)
5. [Code updates](#Code-updates)
6. [Resources](#Resources)

# Getting started

Just a few steps are needed to get started with the example deployment. 
the deployment process is separated in a prerequisites deployment containing the creation of the source file Amazon S3 Bucket and another containing the actual deployment of the infrastructure and application layer. 

You may use the included [setup script](./setup.sh) to simplify and [automatic deployment](#automatic) or alternatively you can run the deployment [step-by-step](#step-by-step). 

## Prerequisites

To run the automated, [setup script](./setup.sh) based deployment you need to have some software installed and configured on your device: 

- bash (zsh, csh, sh should also work, not tested though)
- an [installed and configured ](https://docs.aws.amazon.com/cli/latest/userguide/cli-chap-install.html) aws-cli
- [a named profile](https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-profiles.html) at the aws-cli configuration reflecting the account you are planning to use for the deployment
- [jq](https://stedolan.github.io/jq/)
- [docker](https://www.docker.com/) 

To run the step-by-step setup: 

- an [installed and configured ](https://docs.aws.amazon.com/cli/latest/userguide/cli-chap-install.html) aws-cli

## Parameters

**Dynamic parameters**

These parameters you have to pass to the [setup script](./setup.sh) 

| Parameter Name | Value |
| --- | --- | 
| -p | the aws-cli profile to use | 
| -s | the Cloudformation stack name you want to use |


**Deployment parameters:**

The deployment parameters are placed into the aws-param.json or to be set via cli/console ( if you choose the step-by-step setup. )

| Parameter Name | Default Value | Description | Comment |
| ---- | ---- | ---- | ---- |
| TUTECSInstanceType| t3a.large| Instance size of the ECS Cluster worker nodes or "fargate" for serverless deployment | EC2 instance sizes should be aligned with the size VCPU and Memory limits of the to be deployed tasks. setting this parameter to fargate will cause a Serverless Setup using AWS Fargate |
| TUTApplicationRootVolumeSize | 20 | the size of the application root volume |
| TUTDBInstanceType| db.t3.medium| Instance size of the Postgres Database Instance or "serverless" for serverless deployment | Heavily related to usage, collect metrics and test. 
| TUTECSMaxInstances| 10| The maximum amount of instances the ECS cluster should scale out to | set a reasonable maximum to prevent cost explosion on unexpected usage
| TUTECSMinInstances| 1| The minimum amount of worker instances at the ECS cluster| 
| TUTECSDesiredInstances| 3| The desired amount of instances of worker instances at the ECS cluster |
| TUTDBName| frontendapp| Set a Database Name for the tutorial | 
| TUTDBEngineVersion| 10.7| Set the Postgres version to be used at the Amazon Aurora setup | please refer to the Amazon Aurora [documentation](https://docs.aws.amazon.com/AmazonRDS/latest/AuroraUserGuide/AuroraPostgreSQL.Updates.20180305.html) for supported versions
| TUTEnvironmentStage| dev | can be set to "dev","stage" or "prod" | currently stage or prod does change the Amazon Aurora Setup to a Multi-AZ Setup and adds a 2nd Nat-Gateway to the deployment. 
| TUTServerlessAuroraMinCapacity | The minimum capacity for the Amazon Aurora Serverless Cluster. | Value has to be >= 2
| TUTServerlessAuroraMaxCapacity | The maximum capacity for the Amazon Aurora Serverless Cluster.
| 
# Deployment

## Automatic

For the automatic deployment just run the included [setup script](./setup.sh) 

Example: 
```
./setup.sh -p tut_profile -s tutexample
```

The automatic deployment works as follows: 

- The [setup script](./setup.sh) will validate the device prerequisites are met and all needed parameters are set. 
- It will then validate the syntax of the Amazon Cloudformation templates prior to execute any deployment. 
- It's going to deploy the Amazon S3 Bucket needed by the main deployment and read out the Bucket name as well as the name of the Stack deployed. 
- It will copy the needed scripts, config files for application and services as well as nested templates to the the deployed Bucket. 
- The main deployment will be executed. The script will read the content of the  [aws-param.json](./aws-param.json) file and pass it through the stack deployment

# Step-by-step

If you want to attempt the deployment step-by-step via Console or aws-cli please use the following steps: 

- deploy the Source Amazon S3 Bucket for scripts, config files and nested templates

```
aws cloudformation deploy --stack-name tutexample-sources --profile=tut_profile --template ./templates/aws-buildbuckets.template.yaml
```

- copy the content of the [templates](./templates) into the Source Bucket

````
    aws s3 sync --profile=tut_profile ./templates s3://NAMEOFCREATEDBUCKET
````

- start the deployment using the stackname of the stack deployed beforehand as one of the parameters:

using aws-cli: 
```
aws cloudformation deploy --profile=tut_profile --stack-name tutexample \
    --capabilities CAPABILITY_NAMED_IAM \
    --parameter-overrides TUTStackBucketStack=tutexample-sources \
    --template ./aws-root.template.yaml
```

The deployment will take approx 5 minutes. 

# Template structure and deployment workflow

The Deployment consists of 2 main templates and 2 nested templates. 

## Main templates

- The deployment of prerequisites via [aws-buildbuckets.template.yaml](./templates/aws-buildbuckets.template.yaml)

    The template deploys the Amazon S3 Bucket containing the database deployment as well as the nested templates source files.
---
- The Master Template for the main deployment [aws-master.template.yaml](./aws-master.template.yaml)

    The template initiates the overall deployment of the TUT example deployment. 
---
## Nested templates

- Deploy Amazon Aurora (Postgres): [aws-database.template.yaml](./templates/aws-database.template.yaml)

    *The deployment of Amazon Aurora is needed to provide a database for Tut sample table*
---

## Customizing your TUT deployment

There are several ways how you can further customize your deployment. Apart from the infrastructure components you can customize using the parameters mentioned earlier at the documentation you can also adjust the bootstrap of the TUT deployment according to your needs. 

# Code updates

to update an already deployed stack just pull the current version of the IaC code repository. Afterwards you can start the upgrade process the same way as you would do the initial setup. 

---

# Resources

- AWS Services
    - [Amazon Cloudformation](https://aws.amazon.com/cloudformation/)
    - [Amazon ECS](https://aws.amazon.com/ecs/)
    - [Amazon Aurora](https://aws.amazon.com/rds/aurora/)
    - [Amazon Virtual Private Cloud](https://aws.amazon.com/vpc/)
    - [Amazon S3](https://aws.amazon.com/s3/)