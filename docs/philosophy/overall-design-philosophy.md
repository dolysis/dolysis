# Overall Design Philosophy <!-- omit in toc -->

This is a set of generic design ideals.

## Sections <!-- omit in toc -->

- [The Big Three](#the-big-three)
  - [Simple Should Be Simple](#simple-should-be-simple)
  - [Complex Should Be Possible](#complex-should-be-possible)
  - [Transition Must Be Easy](#transition-must-be-easy)
- [Other Considerations](#other-considerations)
  - [Practical Trumps Ideal](#practical-trumps-ideal)
  - [Technical Debt Is Real Debt](#technical-debt-is-real-debt)
  - [You Cannot Simplify By Adding Complexity](#you-cannot-simplify-by-adding-complexity)
  - [Never Add A Feature You Don't Need](#never-add-a-feature-you-dont-need)
  - [It Is Cheaper To Move Code Than Data](#it-is-cheaper-to-move-code-than-data)
  - [Security Debt Costs 300% More Than Regular Debt](#security-debt-costs-300-more-than-regular-debt)

## The Big Three

To make a comparison, I will use `C++` and `Perl`. This is purely to make an example; these were chosen to demonstrate the point and show that just because something follows the Big Three rules, that does not mean it is good or better.

### Simple Should Be Simple

To start and do something simple, it should be easy and not require a complex configuration.

Think of Docker vs Kubernetes, where doing a simple _hello world_ container for Docker under Ubuntu would be something along the lines of

```bash
# Install Docker
sudo apt install docker
sudo docker run hello-world
```

while [Install Minikube](https://matthewpalmer.net/kubernetes-app-developer/articles/install-kubernetes-ubuntu-tutorial.html) produces the following commands.

```bash
# Step 1
sudo apt-get update
sudo apt-get install -y apt-transport-https
# Step 2
sudo apt-get install -y virtualbox virtualbox-ext-pack
# Step 3
curl -s https://packages.cloud.google.com/apt/doc/apt-key.gpg | sudo apt-key add -
sudo touch /etc/apt/sources.list.d/kubernetes.list
echo "deb http://apt.kubernetes.io/ kubernetes-xenial main" | sudo tee -a /etc/apt/sources.list.d/kubernetes.list
sudo apt-get update
sudo apt-get install -y kubectl
# Step 4
curl -Lo minikube https://storage.googleapis.com/minikube/releases/v0.28.2/minikube-linux-amd64
chmod +x minikube && sudo mv minikube /usr/local/bin/
# Step 5
minikube start
kubectl api-versions
kubectl create deployment hello-minikube --image=k8s.gcr.io/echoserver:1.10
```

As you can see, even a simple Kubernetes setup is way more complex than a Docker setup.

### Complex Should Be Possible

It is possible to do simple things with a solution, but that should not preclude you from be able to do something complex with it.

Both Docker and Kubernetes are able to handle complex environments, though there is an argument to be made that Kubernetes does a better job.

### Transition Must Be Easy

This means that for the transition from simple to complex, most of the configuration is transferable. This is where Docker proves the point, with a simple promotional command `docker swarm init` whereas with Kubernetes it is a completely new configuration with almost nothing in common.

## Other Considerations

### Practical Trumps Ideal

A system that is up and running while performing adequately **is better** than an ideal solution that is down half the time, or needs special care even if it is _technically_ superior.

### Technical Debt Is Real Debt

This means that anything that can be considered _technical debt_ accrues interest, and the cost in fixing it later will be worse that fixing it now.

### You Cannot Simplify By Adding Complexity

While it is possible to abstract complexity, this is not simplification. Abstracting complexity, rather than simplifying, is only beneficial when responsibility for the abstraction can be outsourced. This is _never_ cheaper than simplification.

### Never Add A Feature You Don't Need

This is particularly important for supporting libraries you write. In real terms, this means you should not add more data abstractions than you need to make your code maintainable.

### It Is Cheaper To Move Code Than Data

Move data as little as possible, and move only the data you need. On the whole, integers are better than floats are better than strings. All of JSON/YAML/XML is strings.

### Security Debt Costs 300% More Than Regular Debt

Security done early tends to iron out problems you did not know you had 'til you try to secure your code. Often to properly secure an environment, the code structure needs to be redesigned.
