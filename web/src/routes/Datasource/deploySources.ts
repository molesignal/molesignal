import type { Source } from './sources';

/**
 * Pipelines that report each deployment to `POST /api/v1/apm/deployments`,
 * which puts it on the APM deployment timeline of its service. The version
 * reported is the one the service's spans carry as `service.version`; a
 * repository address with a password in it is kept with the password masked.
 */

const ENDPOINT = '{{ENDPOINT}}';
const TOKEN = '{{TOKEN}}';
const DOCS = 'https://docs.molesignal.com/en-US/apm/deployments';

export const DEPLOYMENT_SOURCES: Source[] = [
  {
    id: 'github-actions',
    category: 'devops',
    signals: ['traces'],
    docsUrl: DOCS,
    steps: [
      { code: { lang: 'bash', content: `gh secret set MOLESIGNAL_TOKEN --body "${TOKEN}"` } },
      {
        code: {
          lang: 'yaml',
          content: `- name: Report the deployment to MoleSignal
  if: always()
  env:
    MOLESIGNAL_TOKEN: \${{ secrets.MOLESIGNAL_TOKEN }}
    DEPLOY_STATUS: \${{ job.status == 'success' && 'succeeded' || 'failed' }}
  run: |
    cat > deployment.json <<EOF
    {
      "service": "checkout",
      "environment": "production",
      "version": "\${GITHUB_SHA::12}",
      "status": "$DEPLOY_STATUS",
      "commit": "$GITHUB_SHA",
      "repository": "$GITHUB_SERVER_URL/$GITHUB_REPOSITORY",
      "author": "$GITHUB_ACTOR",
      "ci_url": "$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID"
    }
    EOF
    curl -fsS -X POST "${ENDPOINT}/api/v1/apm/deployments" \\
      -H "Authorization: Bearer $MOLESIGNAL_TOKEN" \\
      -H "Content-Type: application/json" \\
      --data-binary @deployment.json`,
        },
      },
    ],
  },
  {
    id: 'gitlab-ci',
    category: 'devops',
    signals: ['traces'],
    docsUrl: DOCS,
    steps: [
      {},
      {
        code: {
          lang: 'yaml',
          content: `deploy:
  stage: deploy
  environment: production
  script:
    - ./deploy.sh
  after_script:
    - |
      if [ "$CI_JOB_STATUS" = "success" ]; then DEPLOY_STATUS=succeeded; else DEPLOY_STATUS=failed; fi
      cat > deployment.json <<EOF
      {
        "service": "checkout",
        "environment": "$CI_ENVIRONMENT_NAME",
        "version": "$CI_COMMIT_SHORT_SHA",
        "status": "$DEPLOY_STATUS",
        "commit": "$CI_COMMIT_SHA",
        "repository": "$CI_PROJECT_URL",
        "author": "$GITLAB_USER_LOGIN",
        "ci_url": "$CI_PIPELINE_URL"
      }
      EOF
      curl -fsS -X POST "${ENDPOINT}/api/v1/apm/deployments" \\
        -H "Authorization: Bearer $MOLESIGNAL_TOKEN" \\
        -H "Content-Type: application/json" \\
        --data-binary @deployment.json`,
        },
      },
    ],
  },
  {
    id: 'jenkins',
    category: 'devops',
    signals: ['traces'],
    docsUrl: DOCS,
    steps: [
      {},
      {
        code: {
          lang: 'groovy',
          content: `post {
  always {
    script {
      writeFile file: 'deployment.json', text: groovy.json.JsonOutput.toJson([
        service: 'checkout',
        environment: 'production',
        version: env.GIT_COMMIT.take(12),
        status: currentBuild.currentResult == 'SUCCESS' ? 'succeeded' : 'failed',
        commit: env.GIT_COMMIT,
        repository: env.GIT_URL,
        ci_url: env.BUILD_URL,
      ])
    }
    withCredentials([string(credentialsId: 'molesignal-token', variable: 'MOLESIGNAL_TOKEN')]) {
      sh '''
        curl -fsS -X POST "${ENDPOINT}/api/v1/apm/deployments" \\
          -H "Authorization: Bearer $MOLESIGNAL_TOKEN" \\
          -H "Content-Type: application/json" \\
          --data-binary @deployment.json
      '''
    }
  }
}`,
        },
      },
    ],
  },
];
